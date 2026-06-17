use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ai::skills::{home_skills_path, SkillProvider, SkillScope, SKILL_PROVIDER_DEFINITIONS};
use strum::IntoEnumIterator;
use warp_core::send_telemetry_from_ctx;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warp_util::path::LineAndColumnArg;
use warpui::elements::{
    resizable_state_handle, Border, ChildView, ClippedScrollStateHandle, ClippedScrollable,
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DragBarSide, Element, Empty,
    EventHandler, Fill as ElementFill, Flex, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, Resizable, ResizableStateHandle, SavePosition, ScrollbarWidth,
    Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Entity, FocusContext, ModelHandle, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WeakViewHandle,
};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_conversations_model::AgentConversationsModel;
use crate::ai::mcp::{
    FileBasedMCPManager, MCPProvider, MCPServerState, TemplatableMCPServerManager,
};
use crate::ai::skills::{SkillDescriptor, SkillManager};
use crate::appearance::Appearance;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::CloudObject;
use crate::code::buffer_location::LocalOrRemotePath;
#[cfg(feature = "local_fs")]
use crate::code::file_tree::FileTreeEvent;
use crate::code::file_tree::FileTreeView;
use crate::coding_panel_enablement_state::CodingPanelEnablementState;
use crate::drive::panel::{
    DrivePanel, DrivePanelEvent, MAX_SIDEBAR_WIDTH_RATIO, MIN_SIDEBAR_WIDTH,
};
use crate::editor::{
    EditorOptions, EditorView, EnterAction, EnterSettings, Event as EditorEvent, TextColors,
    TextOptions,
};
use crate::pane_group::pane::view::header::components::HEADER_EDGE_PADDING;
use crate::pane_group::pane::view::header::PANE_HEADER_HEIGHT;
use crate::pane_group::working_directories::WorkingDirectory;
use crate::pane_group::{
    PaneGroup, WorkingDirectoriesEvent, WorkingDirectoriesModel, {self},
};
use crate::server::ids::SyncId;
#[cfg(feature = "local_fs")]
use crate::server::telemetry::CodePanelsFileOpenEntrypoint;
use crate::server::telemetry::{FileTreeSource, WarpDriveSource};
use crate::settings::{AISettings, AISettingsChangedEvent, CLIAgentBuiltinPromptMode};
use crate::settings_view::keybindings::{KeybindingChangedEvent, KeybindingChangedNotifier};
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::resizable_data::{ModalType, ResizableData};
use crate::terminal::CLIAgent;
use crate::ui_components::buttons::{icon_button, icon_button_with_color};
use crate::ui_components::icons;
use crate::util::bindings::keybinding_name_to_display_string;
#[cfg(feature = "local_fs")]
use crate::util::file::external_editor::EditorSettings;
#[cfg(feature = "local_fs")]
use crate::util::openable_file_type::resolve_file_target_with_editor_choice;
use crate::util::openable_file_type::FileTarget;
use crate::workspace::action::ToolConfigScope;
use crate::workspace::view::conversation_list::view::{
    ConversationListView, Event as ConversationListViewEvent,
};
use crate::workspace::view::global_search::view::{
    Event as GlobalSearchViewEvent, GlobalSearchEntryFocus, GlobalSearchView,
};
use crate::workspace::view::ssh_remote::{
    SshRemoteHost, SshRemoteModel, SshRemoteView, SshRemoteViewEvent,
};
use crate::workspace::view::{
    LEFT_PANEL_AGENT_CONVERSATIONS_BINDING_NAME, LEFT_PANEL_GLOBAL_SEARCH_BINDING_NAME,
    LEFT_PANEL_PROJECT_EXPLORER_BINDING_NAME, LEFT_PANEL_SSH_REMOTE_BINDING_NAME,
    LEFT_PANEL_WARP_DRIVE_BINDING_NAME, OPEN_GLOBAL_SEARCH_BINDING_NAME,
    SSH_REMOTE_PANEL_POSITION_ID, TOGGLE_CONVERSATION_LIST_VIEW_BINDING_NAME,
    TOGGLE_PROJECT_EXPLORER_BINDING_NAME, TOGGLE_SSH_REMOTE_BINDING_NAME,
    TOGGLE_WARP_DRIVE_BINDING_NAME,
};
use crate::workspace::WorkspaceAction;
use crate::TelemetryEvent;

#[derive(Default)]
struct MouseStateHandles {
    tools_configurations_button: MouseStateHandle,
    project_explorer_button: MouseStateHandle,
    conversation_list_view_button: MouseStateHandle,
    ssh_remote_button: MouseStateHandle,
    global_search_button: MouseStateHandle,
    warp_drive_button: MouseStateHandle,
}

#[derive(Clone, Debug)]
pub enum LeftPanelAction {
    ToolConfigurations,
    SelectToolsConfigTab(ToolsConfigTab),
    SelectToolsProviderFilter(ToolsProviderFilter),
    SelectSkillConfigFilter(SkillConfigFilter),
    ProjectExplorer,
    GlobalSearch { entry_focus: GlobalSearchEntryFocus },
    WarpDrive,
    ConversationListView,
    SshRemote,
}

#[allow(clippy::large_enum_variant)]
pub enum LeftPanelEvent {
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    FileTree(pane_group::Event),
    WarpDrive(DrivePanelEvent),
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    OpenFileWithTarget {
        location: LocalOrRemotePath,
        target: FileTarget,
        line_col: Option<LineAndColumnArg>,
    },
    NewConversationInNewTab,
    ConnectSshRemoteHost(String),
    DisconnectSshRemoteHost(String),
    ShowDeleteConfirmationDialog {
        conversation_id: AIConversationId,
        conversation_title: String,
        terminal_view_id: Option<warpui::EntityId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPanelView {
    ToolConfigurations,
    ProjectExplorer,
    GlobalSearch { entry_focus: GlobalSearchEntryFocus },
    WarpDrive,
    ConversationListView,
    SshRemote,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ToolsConfigTab {
    Prompts,
    Mcp,
    Skills,
}

impl ToolsConfigTab {
    const ALL: [Self; 3] = [Self::Prompts, Self::Mcp, Self::Skills];

    fn label(self) -> &'static str {
        match self {
            Self::Prompts => "Prompts",
            Self::Mcp => "MCP",
            Self::Skills => "Skills",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Prompts => Icon::Prompt,
            Self::Mcp => Icon::Dataflow,
            Self::Skills => Icon::Folder,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ToolsProviderFilter {
    All,
    Claude,
    Codex,
    OpenCode,
    Warp,
    Gemini,
    Agents,
}

impl ToolsProviderFilter {
    const ALL: [Self; 7] = [
        Self::All,
        Self::Claude,
        Self::Codex,
        Self::OpenCode,
        Self::Warp,
        Self::Gemini,
        Self::Agents,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Warp => "Warp",
            Self::Gemini => "Gemini",
            Self::Agents => "Agents",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::All => Icon::Settings,
            Self::Claude => Icon::ClaudeLogo,
            Self::Codex => Icon::OpenAILogo,
            Self::OpenCode => Icon::OpenCodeLogo,
            Self::Warp | Self::Agents => Icon::WarpLogoLight,
            Self::Gemini => Icon::GeminiLogo,
        }
    }

    fn cli_agent(self) -> Option<CLIAgent> {
        match self {
            Self::Claude => Some(CLIAgent::Claude),
            Self::Codex => Some(CLIAgent::Codex),
            Self::OpenCode => Some(CLIAgent::OpenCode),
            _ => None,
        }
    }

    fn skill_provider(self) -> Option<SkillProvider> {
        match self {
            Self::Claude => Some(SkillProvider::Claude),
            Self::Codex => Some(SkillProvider::Codex),
            Self::OpenCode => Some(SkillProvider::OpenCode),
            Self::Warp => Some(SkillProvider::Warp),
            Self::Gemini => Some(SkillProvider::Gemini),
            Self::Agents => Some(SkillProvider::Agents),
            Self::All => None,
        }
    }

    fn mcp_provider(self) -> Option<MCPProvider> {
        match self {
            Self::Claude => Some(MCPProvider::Claude),
            Self::Codex => Some(MCPProvider::Codex),
            Self::Warp => Some(MCPProvider::Warp),
            Self::OpenCode | Self::Gemini | Self::Agents => Some(MCPProvider::Agents),
            Self::All => None,
        }
    }

    fn matches_cli_agent(self, agent: CLIAgent) -> bool {
        self.cli_agent().is_none_or(|filter| filter == agent)
    }

    fn matches_skill_provider(self, provider: SkillProvider) -> bool {
        self.skill_provider()
            .is_none_or(|filter| filter == provider)
    }

    fn matches_mcp_provider(self, provider: MCPProvider) -> bool {
        self.mcp_provider().is_none_or(|filter| filter == provider)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum SkillConfigFilter {
    Project,
    Home,
    Bundled,
    All,
}

impl SkillConfigFilter {
    const ALL: [Self; 4] = [Self::Project, Self::Home, Self::Bundled, Self::All];

    fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Home => "Home",
            Self::Bundled => "Bundled",
            Self::All => "All",
        }
    }

    fn matches(self, skill: &SkillDescriptor) -> bool {
        match self {
            Self::Project => matches!(skill.scope, SkillScope::Project),
            Self::Home => matches!(skill.scope, SkillScope::Home),
            Self::Bundled => matches!(skill.scope, SkillScope::Bundled),
            Self::All => true,
        }
    }
}

#[derive(Clone)]
struct ToolsRowAction {
    label: &'static str,
    icon: Icon,
    action: WorkspaceAction,
}

struct PromptWorkflowSummary {
    id: SyncId,
    name: String,
    prompt_preview: String,
    breadcrumbs: String,
}

/// Encapsulates the active view state to enforce that all mutations go through
/// `active_view_state::set`, which handles necessary side effects.
mod active_view_state {
    use warpui::ViewContext;

    use super::ToolPanelView;

    pub struct ActiveViewState(ToolPanelView);

    impl ActiveViewState {
        pub fn get(&self) -> ToolPanelView {
            self.0
        }
    }

    pub fn new(view: ToolPanelView) -> ActiveViewState {
        ActiveViewState(view)
    }

    pub fn set(
        left_panel: &mut super::LeftPanelView,
        new_view: ToolPanelView,
        ctx: &mut ViewContext<super::LeftPanelView>,
    ) {
        let previous = left_panel.active_view.0;
        left_panel.active_view.0 = new_view;
        left_panel.update_button_active_states();
        ctx.notify();

        let was_conversation_list_open = previous == ToolPanelView::ConversationListView;
        let is_conversation_list_open = new_view == ToolPanelView::ConversationListView;
        if was_conversation_list_open && !is_conversation_list_open {
            left_panel.on_conversation_list_view_visibility_changed(false, ctx);
        } else if !was_conversation_list_open && is_conversation_list_open {
            left_panel.on_conversation_list_view_visibility_changed(true, ctx);
        }

        left_panel.update_active_file_tree_subscription_state(ctx);
    }
}

pub struct ToolbeltButtonConfig {
    pub icon: warp_core::ui::Icon,
    /// Optional icon to use when the given toolbelt option is in an active state.
    pub active_icon: Option<warp_core::ui::Icon>,
    pub tooltip_text: String,
    pub action: LeftPanelAction,
    /// Whether the button should be rendered with an "active" state.
    pub render_with_active_state: bool,
    /// Ordered list of binding names used to populate the tooltip keybinding display.
    ///
    /// Earlier bindings in the list are preferred in the tooltip.
    pub tooltip_keybinding_names: Vec<&'static str>,
    /// Cached keybinding display string for the tooltip.
    ///
    /// This is updated in response to [`KeybindingChangedEvent`]s.
    pub tooltip_keybinding: Option<String>,
}

pub struct LeftPanelView {
    resizable_state_handle: ResizableStateHandle,
    mouse_state_handles: MouseStateHandles,
    close_button_mouse_state: MouseStateHandle,
    tools_config_scroll_state: ClippedScrollStateHandle,
    tools_config_tab: ToolsConfigTab,
    tools_provider_filter: ToolsProviderFilter,
    skill_config_filter: SkillConfigFilter,
    cli_agent_builtin_prompt_editors: Vec<(CLIAgent, ViewHandle<EditorView>)>,
    warp_drive_view: ViewHandle<DrivePanel>,
    conversation_list_view: ViewHandle<ConversationListView>,
    ssh_remote_view: ViewHandle<SshRemoteView>,
    active_view: active_view_state::ActiveViewState,
    toolbelt_buttons: Vec<ToolbeltButtonConfig>,
    active_pane_group: Option<WeakViewHandle<PaneGroup>>,
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    working_directories_model: ModelHandle<WorkingDirectoriesModel>,
    is_agent_management_view_open: bool,
    panel_position: super::PanelPosition,
    close_action: WorkspaceAction,
}

fn toolbelt_tooltip_keybinding(binding_names: &[&'static str], app: &AppContext) -> Option<String> {
    let mut parts = Vec::new();
    let mut seen = HashSet::new();

    // Preserve caller-provided ordering so we can prioritize specific bindings.
    for binding_name in binding_names {
        if let Some(displayed) = keybinding_name_to_display_string(binding_name, app) {
            if seen.insert(displayed.clone()) {
                parts.push(displayed);
            }
        }
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

impl LeftPanelView {
    fn create_cli_agent_builtin_prompt_editor(
        agent: CLIAgent,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let initial_prompt = AISettings::as_ref(ctx)
            .cli_agent_builtin_prompt(agent)
            .prompt
            .clone();
        let editor = ctx.add_typed_action_view(move |ctx| {
            let appearance = Appearance::handle(ctx).as_ref(ctx);
            let options = EditorOptions {
                autogrow: true,
                soft_wrap: true,
                placeholder_soft_wrap: true,
                enter_settings: EnterSettings {
                    enter: EnterAction::Emit,
                    ..Default::default()
                },
                text: TextOptions {
                    font_size_override: Some(appearance.ui_font_size()),
                    font_family_override: Some(appearance.monospace_font_family()),
                    text_colors_override: Some(TextColors {
                        default_color: appearance.theme().active_ui_text_color(),
                        disabled_color: appearance.theme().disabled_ui_text_color(),
                        hint_color: appearance.theme().disabled_ui_text_color(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::new(options, ctx);
            editor.set_placeholder_text(
                format!("Custom system prompt for {}", agent.display_name()),
                ctx,
            );
            editor.set_buffer_text(&initial_prompt, ctx);
            editor
        });

        ctx.subscribe_to_view(&editor, move |_, editor, event, ctx| match event {
            EditorEvent::Blurred | EditorEvent::Enter => {
                let prompt = editor.as_ref(ctx).buffer_text(ctx);
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.set_cli_agent_builtin_prompt_text(agent, prompt, ctx);
                });
            }
            _ => {}
        });

        editor
    }

    fn sync_cli_agent_builtin_prompt_editors(&mut self, ctx: &mut ViewContext<Self>) {
        for (agent, editor) in &self.cli_agent_builtin_prompt_editors {
            let prompt = AISettings::as_ref(ctx)
                .cli_agent_builtin_prompt(*agent)
                .prompt;
            editor.update(ctx, |editor, ctx| {
                if editor.buffer_text(ctx) != prompt {
                    editor.system_reset_buffer_text(&prompt, ctx);
                }
            });
        }
    }

    fn ssh_remote_file_tree_root(
        pane_group: &ViewHandle<PaneGroup>,
        host: &SshRemoteHost,
        ctx: &AppContext,
    ) -> String {
        pane_group
            .as_ref(ctx)
            .active_session_view(ctx)
            .and_then(|terminal| terminal.as_ref(ctx).pwd())
            .filter(|pwd| !pwd.trim().is_empty())
            .unwrap_or_else(|| {
                let setup_dir = host.remote_setup_dir.trim();
                if setup_dir.is_empty() {
                    "/".to_owned()
                } else {
                    setup_dir.to_owned()
                }
            })
    }

    pub fn new(
        working_directories_model: ModelHandle<WorkingDirectoriesModel>,
        views: Vec<ToolPanelView>,
        close_action: WorkspaceAction,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let resizable_data_handle = ResizableData::handle(ctx);
        let resizable_state_handle = match resizable_data_handle
            .as_ref(ctx)
            .get_handle(ctx.window_id(), ModalType::LeftPanelWidth)
        {
            Some(handle) => handle,
            None => {
                log::error!("Couldn't retrieve left panel resizable state handle.");
                resizable_state_handle(600.0)
            }
        };
        let warp_drive_view = ctx.add_typed_action_view(DrivePanel::new);
        let conversation_list_view = ctx.add_typed_action_view(ConversationListView::new);
        let ssh_remote_view = ctx.add_typed_action_view(SshRemoteView::new);

        ctx.subscribe_to_view(&warp_drive_view, |_me, _, event, ctx| {
            ctx.emit(LeftPanelEvent::WarpDrive(event.clone()));
        });

        ctx.subscribe_to_view(&conversation_list_view, |_me, _, event, ctx| match event {
            ConversationListViewEvent::NewConversationInNewTab => {
                ctx.emit(LeftPanelEvent::NewConversationInNewTab);
            }
            ConversationListViewEvent::ShowDeleteConfirmationDialog {
                conversation_id,
                conversation_title,
                terminal_view_id,
            } => {
                ctx.emit(LeftPanelEvent::ShowDeleteConfirmationDialog {
                    conversation_id: *conversation_id,
                    conversation_title: conversation_title.clone(),
                    terminal_view_id: *terminal_view_id,
                });
            }
        });

        ctx.subscribe_to_view(&ssh_remote_view, |_me, _, event, ctx| match event {
            SshRemoteViewEvent::ConnectHost(host_id) => {
                ctx.emit(LeftPanelEvent::ConnectSshRemoteHost(host_id.clone()));
            }
            SshRemoteViewEvent::DisconnectHost(host_id) => {
                ctx.emit(LeftPanelEvent::DisconnectSshRemoteHost(host_id.clone()));
            }
        });

        let cli_agent_builtin_prompt_editors = AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .map(|agent| {
                (
                    agent,
                    Self::create_cli_agent_builtin_prompt_editor(agent, ctx),
                )
            })
            .collect::<Vec<_>>();

        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
            if matches!(event, AISettingsChangedEvent::CLIAgentBuiltinPrompts { .. }) {
                me.sync_cli_agent_builtin_prompt_editors(ctx);
                ctx.notify();
            }
        });

        let active_view = views
            .first()
            .copied()
            .unwrap_or(ToolPanelView::ToolConfigurations);
        let toolbelt_buttons = views
            .iter()
            .map(|view| Self::create_toolbelt_button_config(view, ctx))
            .collect();

        ctx.subscribe_to_model(
            &KeybindingChangedNotifier::handle(ctx),
            |me, _, event, ctx| match event {
                KeybindingChangedEvent::BindingChanged { .. } => {
                    for button in &mut me.toolbelt_buttons {
                        button.tooltip_keybinding =
                            toolbelt_tooltip_keybinding(&button.tooltip_keybinding_names, ctx);
                    }

                    ctx.notify();
                }
            },
        );

        ctx.subscribe_to_model(&working_directories_model, |me, _, event, ctx| {
            if let WorkingDirectoriesEvent::DirectoriesChanged {
                pane_group_id,
                directories,
            } = event
            {
                let Some(active_pane_group) = &me.active_pane_group else {
                    return;
                };
                let Some(active_pane_group) = active_pane_group.upgrade(ctx) else {
                    return;
                };
                if active_pane_group.id() != *pane_group_id {
                    return;
                }
                let has_terminal_session = directories.iter().any(|dir| dir.terminal_id.is_some());

                let active_ssh_host = SshRemoteModel::as_ref(ctx).active_host().cloned();
                let ssh_remote_root = active_ssh_host
                    .as_ref()
                    .map(|host| Self::ssh_remote_file_tree_root(&active_pane_group, host, ctx));

                // Split directories into local and remote. Embedded SSH remotes
                // use an SFTP-backed tree instead of local cwd/repo metadata.
                let local_paths: Vec<PathBuf> = if active_ssh_host.is_some() {
                    Vec::new()
                } else {
                    directories
                        .iter()
                        .filter_map(|d| d.path.to_local_path().map(|p| p.to_path_buf()))
                        .collect()
                };
                #[allow(unused_variables)]
                let remote_repos: Vec<repo_metadata::RemoteRepositoryIdentifier> =
                    if active_ssh_host.is_some() {
                        Vec::new()
                    } else {
                        directories
                            .iter()
                            .filter_map(|d| match &d.path {
                                LocalOrRemotePath::Remote(remote_path) => {
                                    Some(repo_metadata::RemoteRepositoryIdentifier::new(
                                        remote_path.host_id.clone(),
                                        remote_path.path.clone(),
                                    ))
                                }
                                _ => None,
                            })
                            .collect()
                    };

                // Update GlobalSearchView root directories (local only).
                let global_search_view =
                    me.get_or_create_global_search_view_for_pane_group(active_pane_group.id(), ctx);
                global_search_view.update(ctx, |view, view_ctx| {
                    view.set_root_directories(local_paths.clone(), view_ctx);
                });

                // Directories are already in display order (most recent first) from the model
                let local_directories = deduplicate_by_directory_name(local_paths);
                let file_tree_view =
                    me.get_or_create_file_tree_view_for_pane_group(active_pane_group.id(), ctx);

                let is_visible =
                    active_pane_group.as_ref(ctx).left_panel_open && me.is_file_tree_active();
                file_tree_view.update(ctx, |view, ctx| {
                    if let (Some(host), Some(root)) =
                        (active_ssh_host.clone(), ssh_remote_root.clone())
                    {
                        view.set_root_directories(Vec::new(), ctx);
                        #[cfg(feature = "local_fs")]
                        view.set_remote_root_directories(&[], ctx);
                        view.set_ssh_remote_root_directory(host, root, ctx);
                    } else {
                        view.clear_ssh_remote_root_directories(ctx);
                        view.set_root_directories(local_directories, ctx);
                        #[cfg(feature = "local_fs")]
                        view.set_remote_root_directories(&remote_repos, ctx);
                    }
                    view.set_has_terminal_session(has_terminal_session, ctx);
                    view.set_is_active(is_visible, ctx);

                    if is_visible {
                        view.auto_expand_to_most_recent_directory(ctx);
                    }
                });
                ctx.notify();
            }
        });

        let mut view = Self {
            resizable_state_handle,
            mouse_state_handles: Default::default(),
            close_button_mouse_state: Default::default(),
            tools_config_scroll_state: ClippedScrollStateHandle::default(),
            tools_config_tab: ToolsConfigTab::Prompts,
            tools_provider_filter: ToolsProviderFilter::All,
            skill_config_filter: SkillConfigFilter::Project,
            cli_agent_builtin_prompt_editors,
            warp_drive_view,
            conversation_list_view,
            ssh_remote_view,
            active_view: active_view_state::new(active_view),
            toolbelt_buttons,
            active_pane_group: None,
            working_directories_model,
            is_agent_management_view_open: false,
            panel_position: super::PanelPosition::Left,
            close_action,
        };
        view.update_button_active_states();

        view
    }

    pub fn set_agent_management_view_open(&mut self, is_open: bool, ctx: &mut ViewContext<Self>) {
        self.is_agent_management_view_open = is_open;
        ctx.notify();
    }

    pub fn set_panel_position(
        &mut self,
        position: super::PanelPosition,
        ctx: &mut ViewContext<Self>,
    ) {
        self.panel_position = position;
        ctx.notify();
    }

    /// Updates the available tool panel views.
    /// If the currently active view is no longer available, switches to the first available view.
    pub fn update_available_views(
        &mut self,
        views: Vec<ToolPanelView>,
        ctx: &mut ViewContext<Self>,
    ) {
        // Check if the current active view is still available
        let current_view = self.active_view.get();
        let is_current_view_available = views.iter().any(|v| {
            // Use discriminant comparison for GlobalSearch since it has inner data
            match (v, &current_view) {
                (ToolPanelView::GlobalSearch { .. }, ToolPanelView::GlobalSearch { .. }) => true,
                _ => std::mem::discriminant(v) == std::mem::discriminant(&current_view),
            }
        });

        // Rebuild toolbelt buttons
        self.toolbelt_buttons = views
            .iter()
            .map(|view| Self::create_toolbelt_button_config(view, ctx))
            .collect();

        // If current view is no longer available, switch to the first available view
        if !is_current_view_available {
            if let Some(first_view) = views.first().copied() {
                active_view_state::set(self, first_view, ctx);
            }
        } else {
            self.update_button_active_states();
        }

        ctx.notify();
    }

    fn create_toolbelt_button_config(
        view: &ToolPanelView,
        ctx: &ViewContext<Self>,
    ) -> ToolbeltButtonConfig {
        match view {
            ToolPanelView::ToolConfigurations => {
                let tooltip_keybinding_names = vec!["workspace:toggle_tools_panel"];

                ToolbeltButtonConfig {
                    icon: Icon::Tool2,
                    active_icon: Some(Icon::Tool2),
                    tooltip_text: "Tools".to_string(),
                    action: LeftPanelAction::ToolConfigurations,
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
            ToolPanelView::ProjectExplorer => {
                let tooltip_keybinding_names = vec![
                    LEFT_PANEL_PROJECT_EXPLORER_BINDING_NAME,
                    TOGGLE_PROJECT_EXPLORER_BINDING_NAME,
                ];

                ToolbeltButtonConfig {
                    icon: Icon::FileCopy,
                    active_icon: None,
                    tooltip_text: "Project explorer".to_string(),
                    action: LeftPanelAction::ProjectExplorer,
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
            ToolPanelView::GlobalSearch { .. } => {
                let tooltip_keybinding_names = vec![
                    LEFT_PANEL_GLOBAL_SEARCH_BINDING_NAME,
                    OPEN_GLOBAL_SEARCH_BINDING_NAME,
                ];

                ToolbeltButtonConfig {
                    icon: Icon::Search,
                    active_icon: None,
                    tooltip_text: "Global search".to_string(),
                    action: LeftPanelAction::GlobalSearch {
                        entry_focus: GlobalSearchEntryFocus::QueryEditor,
                    },
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
            ToolPanelView::WarpDrive => {
                let tooltip_keybinding_names = vec![
                    LEFT_PANEL_WARP_DRIVE_BINDING_NAME,
                    TOGGLE_WARP_DRIVE_BINDING_NAME,
                ];

                ToolbeltButtonConfig {
                    icon: Icon::WarpDrive,
                    active_icon: None,
                    tooltip_text: "Warp Drive".to_string(),
                    action: LeftPanelAction::WarpDrive,
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
            ToolPanelView::ConversationListView => {
                let tooltip_keybinding_names = vec![
                    LEFT_PANEL_AGENT_CONVERSATIONS_BINDING_NAME,
                    TOGGLE_CONVERSATION_LIST_VIEW_BINDING_NAME,
                ];

                ToolbeltButtonConfig {
                    icon: Icon::Conversation,
                    active_icon: Some(Icon::Conversation),
                    tooltip_text: "Agent conversations".to_string(),
                    action: LeftPanelAction::ConversationListView,
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
            ToolPanelView::SshRemote => {
                let tooltip_keybinding_names = vec![
                    LEFT_PANEL_SSH_REMOTE_BINDING_NAME,
                    TOGGLE_SSH_REMOTE_BINDING_NAME,
                ];

                ToolbeltButtonConfig {
                    icon: Icon::Cloud,
                    active_icon: Some(Icon::CloudFilled),
                    tooltip_text: "SSH remote".to_string(),
                    action: LeftPanelAction::SshRemote,
                    render_with_active_state: false,
                    tooltip_keybinding: toolbelt_tooltip_keybinding(&tooltip_keybinding_names, ctx),
                    tooltip_keybinding_names,
                }
            }
        }
    }

    fn get_or_create_global_search_view_for_pane_group(
        &mut self,
        pane_group_id: warpui::EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<GlobalSearchView> {
        if let Some(view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_global_search_view(pane_group_id)
        {
            return view;
        }

        let global_search_view = ctx.add_typed_action_view(GlobalSearchView::new);

        ctx.subscribe_to_view(&global_search_view, |me, _, event, ctx| {
            me.handle_global_search_event(event, ctx);
        });

        self.working_directories_model.update(ctx, |model, _ctx| {
            model.store_global_search_view(pane_group_id, global_search_view.clone());
        });

        global_search_view
    }

    fn get_or_create_file_tree_view_for_pane_group(
        &mut self,
        pane_group_id: warpui::EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<FileTreeView> {
        if let Some(view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_file_tree_view(pane_group_id)
        {
            return view;
        }

        let file_tree_view = ctx.add_typed_action_view(FileTreeView::new);

        #[cfg(feature = "local_fs")]
        ctx.subscribe_to_view(&file_tree_view, |me, _, event, ctx| {
            me.handle_file_tree_event(event, ctx);
        });

        self.working_directories_model.update(ctx, |model, _ctx| {
            model.store_file_tree_view(pane_group_id, file_tree_view.clone());
        });

        file_tree_view
    }

    pub fn active_global_search_view(
        &self,
        app: &AppContext,
    ) -> Option<ViewHandle<GlobalSearchView>> {
        let pane_group_id = self
            .active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(app))
            .map(|pane_group| pane_group.id())?;
        self.working_directories_model
            .as_ref(app)
            .get_global_search_view(pane_group_id)
    }

    fn active_file_tree_view(&self, app: &AppContext) -> Option<ViewHandle<FileTreeView>> {
        let pane_group_id = self
            .active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(app))
            .map(|pane_group| pane_group.id())?;
        self.working_directories_model
            .as_ref(app)
            .get_file_tree_view(pane_group_id)
    }

    pub fn active_view(&self) -> ToolPanelView {
        self.active_view.get()
    }

    pub fn is_warp_drive_active(&self) -> bool {
        self.active_view.get() == ToolPanelView::WarpDrive
    }

    pub fn is_file_tree_active(&self) -> bool {
        self.active_view.get() == ToolPanelView::ProjectExplorer
    }

    pub fn warp_drive_view(&self) -> &ViewHandle<DrivePanel> {
        &self.warp_drive_view
    }

    pub(crate) fn auto_expand_active_file_tree_to_most_recent_directory(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(file_tree_view) = self.active_file_tree_view(ctx) {
            file_tree_view.update(ctx, |view, ctx| {
                view.auto_expand_to_most_recent_directory(ctx);
            });
        }
    }

    pub fn restore_active_view_from_snapshot(
        &mut self,
        view: ToolPanelView,
        ctx: &mut ViewContext<Self>,
    ) {
        active_view_state::set(self, view, ctx);
    }

    /// Updates the active pane group ID so we filter events correctly.
    pub fn set_active_pane_group(
        &mut self,
        pane_group: ViewHandle<PaneGroup>,
        working_directories_model: &ModelHandle<WorkingDirectoriesModel>,
        ctx: &mut ViewContext<Self>,
    ) {
        let pane_group_id = pane_group.id();

        let previous_pane_group_id = self
            .active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(ctx))
            .map(|pane_group| pane_group.id());

        self.active_pane_group = Some(pane_group.downgrade());

        if let Some(previous_pane_group_id) = previous_pane_group_id {
            if previous_pane_group_id != pane_group_id {
                self.deactivate_file_tree_view_for_pane_group(previous_pane_group_id, ctx);
            }
        }

        // Query the current state from the model
        let active_directories: Vec<WorkingDirectory> =
            working_directories_model.read(ctx, |model, _| {
                model
                    .most_recent_directories_for_pane_group(pane_group_id)
                    .map(|dirs| dirs.collect())
                    .unwrap_or_default()
            });
        let has_terminal_session = active_directories
            .iter()
            .any(|dir| dir.terminal_id.is_some());

        let active_ssh_host = SshRemoteModel::as_ref(ctx).active_host().cloned();
        let ssh_remote_root = active_ssh_host
            .as_ref()
            .map(|host| Self::ssh_remote_file_tree_root(&pane_group, host, ctx));

        // Split directories into local and remote. Embedded SSH remotes
        // are backed by SFTP directly.
        let local_paths: Vec<PathBuf> = if active_ssh_host.is_some() {
            Vec::new()
        } else {
            active_directories
                .iter()
                .filter_map(|d| d.path.to_local_path().map(|p| p.to_path_buf()))
                .collect()
        };
        #[allow(unused_variables)]
        let remote_repos: Vec<repo_metadata::RemoteRepositoryIdentifier> =
            if active_ssh_host.is_some() {
                Vec::new()
            } else {
                active_directories
                    .iter()
                    .filter_map(|d| match &d.path {
                        LocalOrRemotePath::Remote(remote_path) => {
                            Some(repo_metadata::RemoteRepositoryIdentifier::new(
                                remote_path.host_id.clone(),
                                remote_path.path.clone(),
                            ))
                        }
                        _ => None,
                    })
                    .collect()
            };

        // Update GlobalSearchView root directories (local only).
        let global_search_view =
            self.get_or_create_global_search_view_for_pane_group(pane_group_id, ctx);
        global_search_view.update(ctx, |view, view_ctx| {
            view.set_root_directories(local_paths.clone(), view_ctx);
        });

        let local_directories = deduplicate_by_directory_name(local_paths);
        let active_file_model = pane_group.as_ref(ctx).active_file_model().clone();

        let file_tree_view = self.get_or_create_file_tree_view_for_pane_group(pane_group_id, ctx);
        let left_panel_open = pane_group.as_ref(ctx).left_panel_open;
        let is_visible = left_panel_open && self.is_file_tree_active();
        file_tree_view.update(ctx, |view, ctx| {
            if let (Some(host), Some(root)) = (active_ssh_host.clone(), ssh_remote_root.clone()) {
                view.set_root_directories(Vec::new(), ctx);
                #[cfg(feature = "local_fs")]
                view.set_remote_root_directories(&[], ctx);
                view.set_ssh_remote_root_directory(host, root, ctx);
            } else {
                view.clear_ssh_remote_root_directories(ctx);
                view.set_root_directories(local_directories, ctx);
                #[cfg(feature = "local_fs")]
                view.set_remote_root_directories(&remote_repos, ctx);
            }
            view.set_has_terminal_session(has_terminal_session, ctx);
            view.set_active_file_model(active_file_model, ctx);
            view.set_is_active(is_visible, ctx);

            if is_visible {
                view.auto_expand_to_most_recent_directory(ctx);
            }
        });

        self.on_left_panel_visibility_changed(left_panel_open, ctx);

        ctx.notify();
    }

    pub fn update_coding_panel_enablement(
        &mut self,
        enablement: CodingPanelEnablementState,
        ctx: &mut ViewContext<Self>,
    ) {
        #[cfg(feature = "local_fs")]
        {
            if let Some(file_tree_view) = self.active_file_tree_view(ctx) {
                file_tree_view.update(ctx, |view, ctx| {
                    view.set_enablement_state(enablement, ctx);
                });
            }
        }

        if let Some(global_search_view) = self.active_global_search_view(ctx) {
            global_search_view.update(ctx, |view, view_ctx| {
                view.set_enablement_state(enablement, view_ctx);
            });
        }
    }

    pub fn focus_active_view_on_entry(&mut self, ctx: &mut ViewContext<Self>) {
        match self.active_view.get() {
            ToolPanelView::ToolConfigurations => {
                ctx.focus_self();
            }
            ToolPanelView::ProjectExplorer => {
                if let Some(file_tree_view) = self.active_file_tree_view(ctx) {
                    file_tree_view.update(ctx, |view, ctx| {
                        view.on_left_panel_focused(ctx);
                    });
                    ctx.focus(&file_tree_view);
                }
            }
            ToolPanelView::GlobalSearch { entry_focus } => {
                if let Some(global_search_view) = self.active_global_search_view(ctx) {
                    global_search_view.update(ctx, |view, ctx| {
                        view.on_left_panel_focused(entry_focus, ctx);
                    });
                }

                active_view_state::set(
                    self,
                    ToolPanelView::GlobalSearch {
                        entry_focus: GlobalSearchEntryFocus::Results,
                    },
                    ctx,
                );
            }
            ToolPanelView::WarpDrive => {
                ctx.focus(&self.warp_drive_view);
                self.warp_drive_view.update(ctx, |view, ctx| {
                    view.reset_focused_index_in_warp_drive(true, ctx);
                });
            }
            ToolPanelView::ConversationListView => {
                self.conversation_list_view.update(ctx, |view, ctx| {
                    view.on_left_panel_focused(ctx);
                });
            }
            ToolPanelView::SshRemote => {
                self.ssh_remote_view.update(ctx, |view, ctx| {
                    view.focus_first_field(ctx);
                });
            }
        }
    }

    #[cfg(not(feature = "local_fs"))]
    fn handle_global_search_event(
        &mut self,
        _event: &GlobalSearchViewEvent,
        _ctx: &mut ViewContext<Self>,
    ) {
    }

    #[cfg(feature = "local_fs")]
    fn handle_global_search_event(
        &mut self,
        event: &GlobalSearchViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            GlobalSearchViewEvent::OpenMatch {
                path,
                line_number,
                column_num,
            } => {
                let line_col = LineAndColumnArg {
                    line_num: *line_number as usize,
                    column_num: *column_num,
                };

                let settings = EditorSettings::as_ref(ctx);
                let target = resolve_file_target_with_editor_choice(
                    path,
                    *settings.open_code_panels_file_editor,
                    *settings.prefer_markdown_viewer,
                    *settings.open_file_layout,
                    None,
                );

                send_telemetry_from_ctx!(
                    TelemetryEvent::CodePanelsFileOpened {
                        entrypoint: CodePanelsFileOpenEntrypoint::GlobalSearch,
                        target: target.clone(),
                    },
                    ctx
                );

                ctx.emit(LeftPanelEvent::OpenFileWithTarget {
                    location: LocalOrRemotePath::Local(path.clone()),
                    target,
                    line_col: Some(line_col),
                });
            }
        }
    }

    #[cfg(feature = "local_fs")]
    fn handle_file_tree_event(&mut self, event: &FileTreeEvent, ctx: &mut ViewContext<Self>) {
        match event {
            FileTreeEvent::FileRenamed { old_path, new_path } => {
                ctx.emit(LeftPanelEvent::FileTree(pane_group::Event::FileRenamed {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                }));
            }
            FileTreeEvent::FileDeleted { path } => {
                ctx.emit(LeftPanelEvent::FileTree(pane_group::Event::FileDeleted {
                    path: path.clone(),
                }));
            }
            FileTreeEvent::AttachAsContext { path } => {
                ctx.emit(LeftPanelEvent::FileTree(
                    pane_group::Event::AttachPathAsContext { path: path.clone() },
                ));
            }
            FileTreeEvent::OpenFile {
                path,
                target,
                line_col,
            } => {
                ctx.emit(LeftPanelEvent::OpenFileWithTarget {
                    location: path.clone(),
                    target: target.clone(),
                    line_col: *line_col,
                });
            }
            FileTreeEvent::CDToDirectory { path } => {
                ctx.emit(LeftPanelEvent::FileTree(pane_group::Event::CDToDirectory {
                    path: path.clone(),
                }));
            }
            FileTreeEvent::OpenDirectoryInNewTab { path } => {
                ctx.emit(LeftPanelEvent::FileTree(
                    pane_group::Event::OpenDirectoryInNewTab { path: path.clone() },
                ));
            }
        }
    }
}

impl Entity for LeftPanelView {
    type Event = LeftPanelEvent;
}

impl LeftPanelView {
    fn close_button(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let ui_builder = appearance.ui_builder().clone();
        let tooltip_keybinding =
            keybinding_name_to_display_string("workspace:toggle_left_panel", app);

        let tooltip = if let Some(keybinding) = tooltip_keybinding {
            ui_builder
                .tool_tip_with_sublabel("Close panel".to_string(), keybinding)
                .build()
                .finish()
        } else {
            ui_builder
                .tool_tip("Close panel".to_string())
                .build()
                .finish()
        };

        let icon_color = appearance
            .theme()
            .sub_text_color(appearance.theme().background());
        let close_action = self.close_action.clone();
        icon_button_with_color(
            appearance,
            icons::Icon::X,
            false,
            self.close_button_mouse_state.clone(),
            icon_color,
        )
        .with_tooltip(move || tooltip)
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(close_action.clone());
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn update_button_active_states(&mut self) {
        for button in &mut self.toolbelt_buttons {
            button.render_with_active_state = match &button.action {
                LeftPanelAction::ToolConfigurations => {
                    self.active_view.get() == ToolPanelView::ToolConfigurations
                }
                LeftPanelAction::SelectToolsConfigTab(_) => false,
                LeftPanelAction::SelectToolsProviderFilter(_) => false,
                LeftPanelAction::SelectSkillConfigFilter(_) => false,
                LeftPanelAction::ProjectExplorer => {
                    self.active_view.get() == ToolPanelView::ProjectExplorer
                }
                LeftPanelAction::GlobalSearch { .. } => {
                    matches!(self.active_view.get(), ToolPanelView::GlobalSearch { .. })
                }
                LeftPanelAction::WarpDrive => self.active_view.get() == ToolPanelView::WarpDrive,
                LeftPanelAction::ConversationListView => {
                    self.active_view.get() == ToolPanelView::ConversationListView
                }
                LeftPanelAction::SshRemote => self.active_view.get() == ToolPanelView::SshRemote,
            };
        }
    }

    fn render_small_text(
        text: impl Into<String>,
        size: f32,
        color: impl Into<pathfinder_color::ColorU>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        Text::new_inline(text.into(), appearance.ui_font_family(), size)
            .with_color(color.into())
            .with_clip(ClipConfig::ellipsis())
            .finish()
    }

    fn render_tools_tab_button(
        tab: ToolsConfigTab,
        active: bool,
        count: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let bg = if active {
            internal_colors::fg_overlay_2(theme)
        } else {
            internal_colors::fg_overlay_1(theme)
        };
        let text_color = if active {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let icon_color = text_color;

        let button = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(
                    ConstrainedBox::new(tab.icon().to_warpui_icon(icon_color).finish())
                        .with_width(14.)
                        .with_height(14.)
                        .finish(),
                )
                .with_child(
                    Text::new_inline(tab.label().to_owned(), appearance.ui_font_family(), 12.)
                        .with_color(text_color.into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                )
                .with_child(Self::render_count_chip(count, active, appearance))
                .finish(),
        )
        .with_background(bg)
        .with_border(Border::all(1.).with_border_fill(if active {
            theme.active_ui_detail()
        } else {
            theme.nonactive_ui_detail()
        }))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.)))
        .with_padding_left(8.)
        .with_padding_right(8.)
        .with_padding_top(5.)
        .with_padding_bottom(5.)
        .finish();

        EventHandler::new(button)
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(LeftPanelAction::SelectToolsConfigTab(tab));
                warpui::elements::DispatchEventResult::StopPropagation
            })
            .finish()
    }

    fn render_count_chip(count: usize, active: bool, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let count_label = if count > 99 {
            "99+".to_owned()
        } else {
            count.to_string()
        };
        Container::new(
            Text::new_inline(count_label, appearance.ui_font_family(), 10.)
                .with_color(if active {
                    theme.main_text_color(theme.background()).into()
                } else {
                    theme.sub_text_color(theme.background()).into()
                })
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_background(internal_colors::fg_overlay_3(theme))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(7.)))
        .with_padding_left(6.)
        .with_padding_right(6.)
        .with_padding_top(1.)
        .with_padding_bottom(1.)
        .finish()
    }

    fn render_tools_tab_bar(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let counts = self.tools_tab_counts(app);
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);

        for tab in ToolsConfigTab::ALL {
            row.add_child(
                Shrinkable::new(
                    1.,
                    Self::render_tools_tab_button(
                        tab,
                        self.tools_config_tab == tab,
                        *counts.get(&tab).unwrap_or(&0),
                        appearance,
                    ),
                )
                .finish(),
            );
        }

        Container::new(row.finish())
            .with_padding_left(12.)
            .with_padding_right(12.)
            .with_padding_top(10.)
            .with_padding_bottom(8.)
            .with_border(
                Border::bottom(1.).with_border_fill(appearance.theme().nonactive_ui_detail()),
            )
            .finish()
    }

    fn render_provider_filter_button(
        filter: ToolsProviderFilter,
        active: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = if active {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let button = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_spacing(5.)
                .with_child(
                    ConstrainedBox::new(filter.icon().to_warpui_icon(text_color).finish())
                        .with_width(12.)
                        .with_height(12.)
                        .finish(),
                )
                .with_child(
                    Text::new_inline(filter.label().to_owned(), appearance.ui_font_family(), 11.)
                        .with_color(text_color.into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .with_clip(ClipConfig::ellipsis())
                        .finish(),
                )
                .finish(),
        )
        .with_background(if active {
            internal_colors::fg_overlay_2(theme)
        } else {
            internal_colors::fg_overlay_1(theme)
        })
        .with_border(Border::all(1.).with_border_fill(if active {
            theme.active_ui_detail()
        } else {
            theme.nonactive_ui_detail()
        }))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_padding_left(7.)
        .with_padding_right(7.)
        .with_padding_top(4.)
        .with_padding_bottom(4.)
        .finish();

        EventHandler::new(button)
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(LeftPanelAction::SelectToolsProviderFilter(filter));
                warpui::elements::DispatchEventResult::StopPropagation
            })
            .finish()
    }

    fn render_tools_provider_filter_bar(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut column = Flex::column().with_spacing(6.);
        for chunk in ToolsProviderFilter::ALL.chunks(4) {
            let mut row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.);
            for filter in chunk {
                row.add_child(
                    Shrinkable::new(
                        1.,
                        Self::render_provider_filter_button(
                            *filter,
                            self.tools_provider_filter == *filter,
                            appearance,
                        ),
                    )
                    .finish(),
                );
            }
            column.add_child(row.finish());
        }

        Container::new(column.finish())
            .with_padding_left(12.)
            .with_padding_right(12.)
            .with_padding_top(8.)
            .with_padding_bottom(8.)
            .with_border(
                Border::bottom(1.).with_border_fill(appearance.theme().nonactive_ui_detail()),
            )
            .finish()
    }

    fn render_tools_config_header(&self, app: &AppContext) -> Box<dyn Element> {
        Flex::column()
            .with_child(self.render_tools_tab_bar(app))
            .with_child(self.render_tools_provider_filter_bar(app))
            .finish()
    }

    fn metric_label(label: String, value: usize) -> String {
        let value = if value > 999 {
            "999+".to_owned()
        } else {
            value.to_string()
        };
        format!("{label}: {value}")
    }

    fn render_metric_chip(
        label: String,
        value: usize,
        icon: Icon,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(5.)
                .with_child(
                    ConstrainedBox::new(
                        icon.to_warpui_icon(theme.sub_text_color(theme.background()))
                            .finish(),
                    )
                    .with_width(12.)
                    .with_height(12.)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(
                        Self::metric_label(label, value),
                        appearance.ui_font_family(),
                        11.,
                    )
                    .with_color(theme.main_text_color(theme.background()).into())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
                )
                .finish(),
        )
        .with_background(internal_colors::fg_overlay_1(theme))
        .with_border(Border::all(1.).with_border_fill(theme.nonactive_ui_detail()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
        .with_padding_left(8.)
        .with_padding_right(8.)
        .with_padding_top(3.)
        .with_padding_bottom(3.)
        .finish()
    }

    fn render_metric_chip_rows(
        items: Vec<(String, usize, Icon)>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let mut column = Flex::column().with_spacing(6.);
        for chunk in items.chunks(3) {
            let mut row = Flex::row().with_spacing(6.);
            for (label, value, icon) in chunk {
                row.add_child(
                    Shrinkable::new(
                        1.,
                        Self::render_metric_chip(label.clone(), *value, *icon, appearance),
                    )
                    .finish(),
                );
            }
            column.add_child(row.finish());
        }
        column.finish()
    }

    fn render_skill_filter_button(
        filter: SkillConfigFilter,
        active: bool,
        count: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = if active {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let button = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_spacing(5.)
                .with_child(
                    Text::new_inline(filter.label().to_owned(), appearance.ui_font_family(), 11.)
                        .with_color(text_color.into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .with_clip(ClipConfig::ellipsis())
                        .finish(),
                )
                .with_child(Self::render_count_chip(count, active, appearance))
                .finish(),
        )
        .with_background(if active {
            internal_colors::fg_overlay_2(theme)
        } else {
            internal_colors::fg_overlay_1(theme)
        })
        .with_border(Border::all(1.).with_border_fill(if active {
            theme.active_ui_detail()
        } else {
            theme.nonactive_ui_detail()
        }))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.)))
        .with_padding_left(7.)
        .with_padding_right(7.)
        .with_padding_top(4.)
        .with_padding_bottom(4.)
        .finish();

        EventHandler::new(button)
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(LeftPanelAction::SelectSkillConfigFilter(filter));
                warpui::elements::DispatchEventResult::StopPropagation
            })
            .finish()
    }

    fn render_skill_filter_bar(
        &self,
        scope_counts: &HashMap<SkillScope, usize>,
        total_count: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let count_for = |filter| match filter {
            SkillConfigFilter::Project => *scope_counts.get(&SkillScope::Project).unwrap_or(&0),
            SkillConfigFilter::Home => *scope_counts.get(&SkillScope::Home).unwrap_or(&0),
            SkillConfigFilter::Bundled => *scope_counts.get(&SkillScope::Bundled).unwrap_or(&0),
            SkillConfigFilter::All => total_count,
        };

        let mut rows = Flex::column().with_spacing(6.);
        for chunk in SkillConfigFilter::ALL.chunks(2) {
            let mut row = Flex::row().with_spacing(6.);
            for filter in chunk {
                row.add_child(
                    Shrinkable::new(
                        1.,
                        Self::render_skill_filter_button(
                            *filter,
                            self.skill_config_filter == *filter,
                            count_for(*filter),
                            appearance,
                        ),
                    )
                    .finish(),
                );
            }
            rows.add_child(row.finish());
        }

        Container::new(rows.finish())
            .with_padding_left(12.)
            .with_padding_right(12.)
            .finish()
    }

    fn render_tools_action_pill(
        action: ToolsRowAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = theme.sub_text_color(theme.background());
        let pill = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.)
                .with_child(
                    ConstrainedBox::new(action.icon.to_warpui_icon(text_color).finish())
                        .with_width(12.)
                        .with_height(12.)
                        .finish(),
                )
                .with_child(
                    Text::new_inline(action.label.to_owned(), appearance.ui_font_family(), 11.)
                        .with_color(text_color.into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                )
                .finish(),
        )
        .with_background(internal_colors::fg_overlay_2(theme))
        .with_border(Border::all(1.).with_border_fill(theme.nonactive_ui_detail()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_padding_left(6.)
        .with_padding_right(6.)
        .with_padding_top(4.)
        .with_padding_bottom(4.)
        .finish();

        EventHandler::new(pill)
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.action.clone());
                warpui::elements::DispatchEventResult::StopPropagation
            })
            .finish()
    }

    fn render_tools_management_row(
        title: String,
        subtitle: String,
        icon: Icon,
        status: Option<String>,
        primary_action: Option<WorkspaceAction>,
        actions: Vec<ToolsRowAction>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let icon_color = theme.sub_text_color(theme.background());
        let title_color = theme.main_text_color(theme.background());
        let subtitle_color = theme.sub_text_color(theme.background());

        let mut trailing = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_spacing(6.);
        if let Some(status) = status {
            trailing.add_child(
                Container::new(
                    Text::new_inline(status, appearance.ui_font_family(), 10.)
                        .with_color(theme.sub_text_color(theme.background()).into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .with_clip(ClipConfig::ellipsis())
                        .finish(),
                )
                .with_background(internal_colors::fg_overlay_2(theme))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .with_padding_left(6.)
                .with_padding_right(6.)
                .with_padding_top(2.)
                .with_padding_bottom(2.)
                .finish(),
            );
        }
        for action in actions {
            trailing.add_child(Self::render_tools_action_pill(action, appearance));
        }

        let row = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(10.)
                .with_child(
                    ConstrainedBox::new(icon.to_warpui_icon(icon_color).finish())
                        .with_width(16.)
                        .with_height(16.)
                        .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Flex::column()
                            .with_spacing(2.)
                            .with_child(
                                Text::new_inline(title, appearance.ui_font_family(), 12.)
                                    .with_color(title_color.into())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                            )
                            .with_child(Self::render_small_text(
                                subtitle,
                                11.,
                                subtitle_color,
                                appearance,
                            ))
                            .finish(),
                    )
                    .finish(),
                )
                .with_child(trailing.finish())
                .finish(),
        )
        .with_background(theme.background())
        .with_border(Border::bottom(1.).with_border_fill(theme.nonactive_ui_detail()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(0.)))
        .with_padding_left(10.)
        .with_padding_right(8.)
        .with_padding_top(7.)
        .with_padding_bottom(7.)
        .finish();

        if let Some(action) = primary_action {
            EventHandler::new(row)
                .on_left_mouse_down(move |ctx, _, _| {
                    ctx.dispatch_typed_action(action.clone());
                    warpui::elements::DispatchEventResult::StopPropagation
                })
                .finish()
        } else {
            row
        }
    }

    fn render_tools_section(
        title: &str,
        subtitle: Option<String>,
        children: Vec<Box<dyn Element>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut section = Flex::column().with_spacing(8.);
        let mut header = Flex::column().with_spacing(2.);
        header.add_child(
            Text::new_inline(title.to_owned(), appearance.ui_font_family(), 12.)
                .with_color(theme.main_text_color(theme.background()).into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        );
        if let Some(subtitle) = subtitle {
            header.add_child(Self::render_small_text(
                subtitle,
                11.,
                theme.sub_text_color(theme.background()),
                appearance,
            ));
        }
        section.add_child(header.finish());
        for child in children {
            section.add_child(child);
        }

        Container::new(section.finish())
            .with_padding_left(12.)
            .with_padding_right(12.)
            .with_padding_top(8.)
            .with_padding_bottom(4.)
            .finish()
    }

    fn render_empty_tools_row(message: &str, appearance: &Appearance) -> Box<dyn Element> {
        Self::render_tools_management_row(
            message.to_owned(),
            "No configuration files were detected for the current scope".to_owned(),
            Icon::File,
            None,
            None,
            vec![],
            appearance,
        )
    }

    fn render_prompt_agent_configuration(
        agent: CLIAgent,
        editor: &ViewHandle<EditorView>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let prompt_setting = AISettings::as_ref(app).cli_agent_builtin_prompt(agent);
        let has_custom_prompt = !prompt_setting.is_empty();
        let status = if has_custom_prompt {
            prompt_setting.mode.display_name().to_owned()
        } else {
            "Default".to_owned()
        };
        let summary = if has_custom_prompt {
            prompt_setting
                .prompt
                .trim()
                .lines()
                .next()
                .unwrap_or("Custom prompt")
                .to_owned()
        } else {
            "Vendor default prompt; no custom override".to_owned()
        };

        let header = Self::render_tools_management_row(
            agent.display_name().to_owned(),
            summary,
            agent.icon().unwrap_or(Icon::Prompt),
            Some(status),
            None,
            vec![],
            appearance,
        );

        let mut mode_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        for mode in CLIAgentBuiltinPromptMode::iter() {
            mode_row.add_child(Self::render_tools_action_pill(
                ToolsRowAction {
                    label: mode.display_name(),
                    icon: if mode == prompt_setting.mode {
                        Icon::Check
                    } else {
                        Icon::Settings
                    },
                    action: WorkspaceAction::SetCLIAgentBuiltinPromptMode { agent, mode },
                },
                appearance,
            ));
        }
        if has_custom_prompt {
            mode_row.add_child(Self::render_tools_action_pill(
                ToolsRowAction {
                    label: "Reset",
                    icon: Icon::RefreshCcw,
                    action: WorkspaceAction::ResetCLIAgentBuiltinPrompt { agent },
                },
                appearance,
            ));
        }

        let editor_box = Container::new(
            ConstrainedBox::new(ChildView::new(editor).finish())
                .with_height(78.)
                .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_padding_left(8.)
        .with_padding_right(8.)
        .with_padding_top(6.)
        .with_padding_bottom(6.)
        .finish();

        Container::new(
            Flex::column()
                .with_spacing(6.)
                .with_child(header)
                .with_child(
                    Container::new(mode_row.finish())
                        .with_padding_left(10.)
                        .with_padding_right(10.)
                        .finish(),
                )
                .with_child(
                    Container::new(editor_box)
                        .with_padding_left(10.)
                        .with_padding_right(10.)
                        .finish(),
                )
                .finish(),
        )
        .with_padding_bottom(6.)
        .finish()
    }

    fn compact_path(path: &Path) -> String {
        if let Some(home_dir) = dirs::home_dir() {
            if let Ok(stripped) = path.strip_prefix(&home_dir) {
                if stripped.as_os_str().is_empty() {
                    return "~".to_owned();
                }
                return format!("~/{}", stripped.display());
            }
        }
        path.display().to_string()
    }

    fn active_local_working_directory(&self, app: &AppContext) -> Option<LocalOrRemotePath> {
        self.active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(app))
            .and_then(|pane_group| {
                pane_group
                    .as_ref(app)
                    .active_session_view(app)
                    .and_then(|terminal| terminal.as_ref(app).pwd_if_local(app))
            })
            .map(PathBuf::from)
            .map(LocalOrRemotePath::Local)
    }

    fn active_local_working_directory_path(&self, app: &AppContext) -> Option<PathBuf> {
        self.active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(app))
            .and_then(|pane_group| {
                pane_group
                    .as_ref(app)
                    .active_session_view(app)
                    .and_then(|terminal| terminal.as_ref(app).pwd_if_local(app))
            })
            .map(PathBuf::from)
    }

    fn prompt_workflows(app: &AppContext) -> Vec<PromptWorkflowSummary> {
        let mut workflows = CloudModel::as_ref(app)
            .get_all_active_workflows()
            .filter(|workflow| workflow.model().data.is_agent_mode_workflow())
            .map(|workflow| PromptWorkflowSummary {
                id: workflow.id,
                name: workflow.model().data.name().to_owned(),
                prompt_preview: workflow.model().data.content().trim().to_owned(),
                breadcrumbs: workflow.breadcrumbs(app),
            })
            .collect::<Vec<_>>();
        workflows.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
        workflows
    }

    fn tools_tab_counts(&self, app: &AppContext) -> HashMap<ToolsConfigTab, usize> {
        let builtin_prompt_count = AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .filter(|agent| self.tools_provider_filter.matches_cli_agent(*agent))
            .count();
        let prompts = builtin_prompt_count
            + if self.tools_provider_filter == ToolsProviderFilter::All {
                Self::prompt_workflows(app).len()
            } else {
                0
            };
        let manager = TemplatableMCPServerManager::as_ref(app);
        let file_based_manager = FileBasedMCPManager::as_ref(app);
        let detected_mcp_count = file_based_manager
            .file_based_servers()
            .into_iter()
            .filter(|installation| {
                MCPProvider::iter().any(|provider| {
                    self.tools_provider_filter.matches_mcp_provider(provider)
                        && !file_based_manager
                            .directory_paths_for_installation_and_provider(
                                installation.uuid(),
                                provider,
                            )
                            .is_empty()
                })
            })
            .count();
        let provider_config_count = MCPProvider::iter()
            .filter(|provider| self.tools_provider_filter.matches_mcp_provider(*provider))
            .count();
        let mcps = detected_mcp_count
            + provider_config_count
            + if self.tools_provider_filter == ToolsProviderFilter::All {
                manager.get_installed_templatable_servers().len()
            } else {
                0
            };
        let skills = SkillManager::as_ref(app)
            .get_skills_for_working_directory(
                self.active_local_working_directory(app).as_ref(),
                app,
            )
            .into_iter()
            .filter(|skill| {
                self.skill_config_filter.matches(skill)
                    && self
                        .tools_provider_filter
                        .matches_skill_provider(skill.provider)
            })
            .count();

        HashMap::from([
            (ToolsConfigTab::Prompts, prompts),
            (ToolsConfigTab::Mcp, mcps),
            (ToolsConfigTab::Skills, skills),
        ])
    }

    fn render_prompt_config_panel(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let prompt_workflows = Self::prompt_workflows(app);

        let active_session_info = self
            .active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(app))
            .and_then(|pane_group| pane_group.as_ref(app).active_session_view(app))
            .and_then(|terminal| {
                CLIAgentSessionsModel::as_ref(app)
                    .session(terminal.id())
                    .map(|session| (session.agent, session.received_rich_notification))
            });

        let mut system_prompt_rows = Vec::new();
        if let Some((agent, received_rich_notification)) = active_session_info {
            if self.tools_provider_filter.matches_cli_agent(agent) {
                let agent_name = agent.display_name();
                let subtitle = if received_rich_notification {
                    "Plugin session; runtime prompt state received".to_owned()
                } else {
                    "CLI launch session; prompt override is applied when supported".to_owned()
                };
                system_prompt_rows.push(Self::render_tools_management_row(
                    format!("Active session: {agent_name}"),
                    subtitle,
                    agent.icon().unwrap_or(Icon::Prompt),
                    Some("Runtime".to_owned()),
                    None,
                    vec![],
                    appearance,
                ));
            }
        } else if self.tools_provider_filter == ToolsProviderFilter::All {
            system_prompt_rows.push(Self::render_tools_management_row(
                "No active agent session".to_owned(),
                "Start an agent session to inspect runtime prompt state".to_owned(),
                Icon::Prompt,
                Some("Idle".to_owned()),
                None,
                vec![],
                appearance,
            ));
        }

        for (agent, editor) in &self.cli_agent_builtin_prompt_editors {
            if self.tools_provider_filter.matches_cli_agent(*agent) {
                system_prompt_rows
                    .push(Self::render_prompt_agent_configuration(*agent, editor, app));
            }
        }

        if system_prompt_rows.is_empty() {
            system_prompt_rows.push(Self::render_empty_tools_row(
                "No prompt configuration for this provider",
                appearance,
            ));
        }

        let mut workflow_rows = vec![Self::render_tools_management_row(
            "New prompt".to_owned(),
            "Reusable Agent Mode workflow prompt".to_owned(),
            Icon::Plus,
            None,
            Some(WorkspaceAction::CreatePersonalAIPrompt),
            vec![],
            appearance,
        )];

        if self.tools_provider_filter != ToolsProviderFilter::All {
            workflow_rows.clear();
            workflow_rows.push(Self::render_tools_management_row(
                "Shared prompt workflows".to_owned(),
                "Switch to All to manage reusable Agent Mode workflows".to_owned(),
                Icon::Prompt,
                Some("Shared".to_owned()),
                None,
                vec![],
                appearance,
            ));
        } else if prompt_workflows.is_empty() {
            workflow_rows.push(Self::render_empty_tools_row(
                "No saved prompt workflows",
                appearance,
            ));
        } else {
            for workflow in prompt_workflows.into_iter().take(20) {
                let subtitle = if workflow.prompt_preview.is_empty() {
                    workflow.breadcrumbs
                } else {
                    format!("{} - {}", workflow.breadcrumbs, workflow.prompt_preview)
                };
                workflow_rows.push(Self::render_tools_management_row(
                    workflow.name,
                    subtitle,
                    Icon::Prompt,
                    Some("Prompt".to_owned()),
                    Some(WorkspaceAction::OpenPromptWorkflow {
                        workflow_id: workflow.id,
                    }),
                    vec![
                        ToolsRowAction {
                            label: "Edit",
                            icon: Icon::Pencil,
                            action: WorkspaceAction::OpenPromptWorkflow {
                                workflow_id: workflow.id,
                            },
                        },
                        ToolsRowAction {
                            label: "Delete",
                            icon: Icon::Trash,
                            action: WorkspaceAction::TrashPromptWorkflow {
                                workflow_id: workflow.id,
                            },
                        },
                    ],
                    appearance,
                ));
            }
        }

        let chips = Self::render_metric_chip_rows(
            vec![
                (
                    "Saved".to_owned(),
                    Self::prompt_workflows(app).len(),
                    Icon::Prompt,
                ),
                (
                    "Runtime".to_owned(),
                    usize::from(active_session_info.is_some()),
                    Icon::Settings,
                ),
                (
                    "System".to_owned(),
                    AISettings::cli_agent_builtin_prompt_agents().len(),
                    Icon::Settings,
                ),
            ],
            appearance,
        );

        let mut content = Flex::column().with_spacing(10.);
        content.add_child(
            Container::new(chips)
                .with_padding_left(12.)
                .with_padding_right(12.)
                .finish(),
        );
        content.add_child(Self::render_tools_section(
            "System Prompts",
            Some("Runtime state and per-agent overrides".to_owned()),
            system_prompt_rows,
            appearance,
        ));
        content.add_child(Self::render_tools_section(
            "Prompt Workflows",
            Some("Saved Agent Mode prompts with owner breadcrumbs".to_owned()),
            workflow_rows,
            appearance,
        ));

        Container::new(content.finish())
            .with_background(theme.background())
            .finish()
    }

    fn mcp_state_label(state: Option<MCPServerState>) -> &'static str {
        match state {
            Some(MCPServerState::Running) => "Running",
            Some(MCPServerState::Starting) => "Starting",
            Some(MCPServerState::Authenticating) => "Authenticating",
            Some(MCPServerState::ShuttingDown) => "Stopping",
            Some(MCPServerState::FailedToStart) => "Error",
            Some(MCPServerState::NotRunning) | None => "Installed",
        }
    }

    fn render_mcp_config_panel(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let manager = TemplatableMCPServerManager::as_ref(app);
        let file_based_manager = FileBasedMCPManager::as_ref(app);
        let file_based_servers = file_based_manager.file_based_servers();

        let installed_count = manager.get_installed_templatable_servers().len();
        let detected_count = file_based_servers.len();
        let running_count = manager
            .get_installed_templatable_servers()
            .keys()
            .filter(|uuid| {
                matches!(
                    manager.get_server_state(**uuid),
                    Some(MCPServerState::Running)
                )
            })
            .count();

        let chip_items = vec![
            ("Managed".to_owned(), installed_count, Icon::Dataflow),
            ("Detected".to_owned(), detected_count, Icon::Dataflow02),
            ("Running".to_owned(), running_count, Icon::Play),
        ];
        let chips = Self::render_metric_chip_rows(chip_items, appearance);

        let gateway_rows = vec![Self::render_tools_management_row(
            "Agentwarp managed MCP layer".to_owned(),
            "Runtime state, provider configs, and installed servers".to_owned(),
            Icon::Dataflow,
            Some("Unified".to_owned()),
            None,
            vec![],
            appearance,
        )];

        let provider_rows = MCPProvider::iter()
            .filter(|provider| self.tools_provider_filter.matches_mcp_provider(*provider))
            .map(|provider| {
                let mut actions = vec![
                    ToolsRowAction {
                        label: "Home",
                        icon: Icon::File,
                        action: WorkspaceAction::OpenMCPConfigFile {
                            provider,
                            scope: ToolConfigScope::Home,
                        },
                    },
                    ToolsRowAction {
                        label: "Project",
                        icon: Icon::File,
                        action: WorkspaceAction::OpenMCPConfigFile {
                            provider,
                            scope: ToolConfigScope::Project,
                        },
                    },
                ];
                if provider != MCPProvider::Claude {
                    actions.push(ToolsRowAction {
                        label: "Sync H",
                        icon: Icon::RefreshCcw,
                        action: WorkspaceAction::SyncMCPConfig {
                            source: MCPProvider::Claude,
                            target: provider,
                            scope: ToolConfigScope::Home,
                        },
                    });
                    actions.push(ToolsRowAction {
                        label: "Sync P",
                        icon: Icon::RefreshCcw,
                        action: WorkspaceAction::SyncMCPConfig {
                            source: MCPProvider::Claude,
                            target: provider,
                            scope: ToolConfigScope::Project,
                        },
                    });
                }
                Self::render_tools_management_row(
                    if self.tools_provider_filter == ToolsProviderFilter::OpenCode
                        || self.tools_provider_filter == ToolsProviderFilter::Gemini
                    {
                        format!("{} shared MCP config", self.tools_provider_filter.label())
                    } else {
                        format!("{} MCP config", provider.display_name())
                    },
                    format!(
                        "Home: {} - Project: {}",
                        Self::compact_path(&provider.home_config_path()),
                        Self::compact_path(&provider.project_config_path())
                    ),
                    provider.icon(),
                    Some("Config".to_owned()),
                    None,
                    actions,
                    appearance,
                )
            })
            .collect::<Vec<_>>();

        let mut server_rows = Vec::new();
        let mut installations = manager
            .get_installed_templatable_servers()
            .values()
            .collect::<Vec<_>>();
        installations.sort_by_key(|installation| {
            installation
                .templatable_mcp_server()
                .name
                .to_ascii_lowercase()
        });

        if self.tools_provider_filter == ToolsProviderFilter::All {
            for installation in installations.into_iter().take(20) {
                let uuid = installation.uuid();
                let state = manager.get_server_state(uuid);
                let tool_count = manager.tools_for_server(uuid).len();
                let should_run = !matches!(state, Some(MCPServerState::Running));
                let subtitle = if tool_count == 0 {
                    "No tools reported yet".to_owned()
                } else {
                    format!("{tool_count} tools available through the unified MCP manager")
                };
                server_rows.push(Self::render_tools_management_row(
                    installation.templatable_mcp_server().name.clone(),
                    subtitle,
                    Icon::Dataflow,
                    Some(Self::mcp_state_label(state).to_owned()),
                    None,
                    vec![ToolsRowAction {
                        label: if should_run { "Start" } else { "Stop" },
                        icon: if should_run {
                            Icon::Play
                        } else {
                            Icon::StopFilled
                        },
                        action: WorkspaceAction::ToggleMCPServer {
                            installation_uuid: uuid,
                            should_run,
                        },
                    }],
                    appearance,
                ));
            }
        }

        for installation in file_based_servers.into_iter().take(12) {
            let uuid = installation.uuid();
            let state = manager.get_server_state(uuid);
            let provider = MCPProvider::iter().find(|provider| {
                !file_based_manager
                    .directory_paths_for_installation_and_provider(uuid, *provider)
                    .is_empty()
            });
            let provider_label = provider
                .map(|provider| provider.display_name().to_owned())
                .unwrap_or_else(|| "Provider config".to_owned());
            if let Some(provider) = provider {
                if !self.tools_provider_filter.matches_mcp_provider(provider) {
                    continue;
                }
            }
            let actions = provider
                .map(|provider| {
                    let scope = file_based_manager
                        .directory_paths_for_installation_and_provider(uuid, provider)
                        .first()
                        .and_then(|root| {
                            self.active_local_working_directory_path(app)
                                .map(|cwd| (root, cwd))
                        })
                        .map(|(root, cwd)| {
                            if root == &cwd {
                                ToolConfigScope::Project
                            } else {
                                ToolConfigScope::Home
                            }
                        })
                        .unwrap_or(ToolConfigScope::Home);
                    vec![ToolsRowAction {
                        label: "Config",
                        icon: Icon::File,
                        action: WorkspaceAction::OpenMCPConfigFile { provider, scope },
                    }]
                })
                .unwrap_or_default();
            server_rows.push(Self::render_tools_management_row(
                installation.templatable_mcp_server().name.clone(),
                format!("Detected from {provider_label}; managed without copying config into Warp"),
                Icon::Dataflow02,
                Some(Self::mcp_state_label(state).to_owned()),
                None,
                actions,
                appearance,
            ));
        }

        if server_rows.is_empty() {
            server_rows.push(Self::render_empty_tools_row(
                "No MCP servers detected",
                appearance,
            ));
        }

        let mut content = Flex::column().with_spacing(10.);
        content.add_child(
            Container::new(chips)
                .with_padding_left(12.)
                .with_padding_right(12.)
                .finish(),
        );
        content.add_child(Self::render_tools_section(
            "Unified MCP Management",
            Some(
                "Central management for Warp, Claude, Codex, Gemini, OpenCode, and Agents"
                    .to_owned(),
            ),
            gateway_rows,
            appearance,
        ));
        content.add_child(Self::render_tools_section(
            "Provider Configs",
            Some("Watched home and project config paths".to_owned()),
            provider_rows,
            appearance,
        ));
        content.add_child(Self::render_tools_section(
            "Servers",
            Some("Installed and auto-detected MCP servers managed from one place".to_owned()),
            server_rows,
            appearance,
        ));
        content.finish()
    }

    fn skill_subtitle(skill: &SkillDescriptor) -> String {
        let description = skill.description.trim();
        let provider = skill.provider.to_string();
        let scope = skill.scope.to_string();
        if description.is_empty() {
            format!("{provider} - {scope}")
        } else {
            format!("{provider} - {scope} - {description}")
        }
    }

    fn render_skill_config_panel(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let working_directory = self.active_local_working_directory(app);
        let working_directory_path = self.active_local_working_directory_path(app);
        let mut skills = SkillManager::as_ref(app)
            .get_skills_for_working_directory(working_directory.as_ref(), app);
        skills.sort_by(|a, b| {
            a.provider
                .to_string()
                .cmp(&b.provider.to_string())
                .then_with(|| a.scope.to_string().cmp(&b.scope.to_string()))
                .then_with(|| a.name.cmp(&b.name))
        });

        let mut provider_counts: HashMap<SkillProvider, usize> = HashMap::new();
        let mut scope_counts: HashMap<SkillScope, usize> = HashMap::new();
        for skill in &skills {
            *provider_counts.entry(skill.provider).or_default() += 1;
            if self
                .tools_provider_filter
                .matches_skill_provider(skill.provider)
            {
                *scope_counts.entry(skill.scope).or_default() += 1;
            }
        }
        let total_skill_count = skills
            .iter()
            .filter(|skill| {
                self.tools_provider_filter
                    .matches_skill_provider(skill.provider)
            })
            .count();

        let chips = Self::render_metric_chip_rows(
            vec![
                (
                    "Home".to_owned(),
                    *scope_counts.get(&SkillScope::Home).unwrap_or(&0),
                    Icon::Folder,
                ),
                (
                    "Project".to_owned(),
                    *scope_counts.get(&SkillScope::Project).unwrap_or(&0),
                    Icon::Folder,
                ),
                (
                    "Bundled".to_owned(),
                    *scope_counts.get(&SkillScope::Bundled).unwrap_or(&0),
                    Icon::Warp,
                ),
            ],
            appearance,
        );

        let provider_rows = SKILL_PROVIDER_DEFINITIONS
            .iter()
            .filter(|definition| {
                self.tools_provider_filter
                    .matches_skill_provider(definition.provider)
            })
            .map(|definition| {
                let provider = definition.provider;
                let home_path = home_skills_path(provider);
                let project_path = working_directory_path
                    .as_ref()
                    .map(|cwd| cwd.join(&definition.skills_path));
                let mut actions = Vec::new();
                if let Some(home_path) = &home_path {
                    actions.push(ToolsRowAction {
                        label: "Home",
                        icon: Icon::Folder,
                        action: WorkspaceAction::OpenSkillFolder {
                            path: home_path.clone(),
                        },
                    });
                }
                if let Some(project_path) = &project_path {
                    actions.push(ToolsRowAction {
                        label: "Project",
                        icon: Icon::Folder,
                        action: WorkspaceAction::OpenSkillFolder {
                            path: project_path.clone(),
                        },
                    });
                }
                if provider != SkillProvider::Claude {
                    actions.push(ToolsRowAction {
                        label: "Sync H",
                        icon: Icon::RefreshCcw,
                        action: WorkspaceAction::SyncSkillProvider {
                            source: SkillProvider::Claude,
                            target: provider,
                            scope: ToolConfigScope::Home,
                        },
                    });
                    actions.push(ToolsRowAction {
                        label: "Sync P",
                        icon: Icon::RefreshCcw,
                        action: WorkspaceAction::SyncSkillProvider {
                            source: SkillProvider::Claude,
                            target: provider,
                            scope: ToolConfigScope::Project,
                        },
                    });
                }
                let subtitle = match (&home_path, &project_path) {
                    (Some(home), Some(project)) => format!(
                        "Home: {} - Project: {}",
                        Self::compact_path(home),
                        Self::compact_path(project)
                    ),
                    (Some(home), None) => {
                        format!("Home: {} - Project: no local cwd", Self::compact_path(home))
                    }
                    (None, Some(project)) => format!("Project: {}", Self::compact_path(project)),
                    (None, None) => "No local skill folder is available".to_owned(),
                };
                Self::render_tools_management_row(
                    provider.to_string(),
                    subtitle,
                    provider.icon(),
                    Some(Self::metric_label(
                        "Skills".to_owned(),
                        *provider_counts.get(&provider).unwrap_or(&0),
                    )),
                    None,
                    actions,
                    appearance,
                )
            })
            .collect::<Vec<_>>();

        let visible_skills = skills
            .into_iter()
            .filter(|skill| {
                self.skill_config_filter.matches(skill)
                    && self
                        .tools_provider_filter
                        .matches_skill_provider(skill.provider)
            })
            .collect::<Vec<_>>();
        let mut skill_rows = Vec::new();
        if visible_skills.is_empty() {
            skill_rows.push(Self::render_empty_tools_row(
                "No skills detected",
                appearance,
            ));
        } else {
            for skill in visible_skills.into_iter().take(40) {
                let can_edit = !matches!(skill.scope, SkillScope::Bundled);
                skill_rows.push(Self::render_tools_management_row(
                    format!("/{}", skill.name),
                    Self::skill_subtitle(&skill),
                    skill.icon_override.unwrap_or_else(|| skill.provider.icon()),
                    Some(skill.scope.to_string()),
                    can_edit.then(|| WorkspaceAction::OpenSkill {
                        skill_reference: skill.reference.clone(),
                    }),
                    if can_edit {
                        vec![ToolsRowAction {
                            label: "Open",
                            icon: Icon::Pencil,
                            action: WorkspaceAction::OpenSkill {
                                skill_reference: skill.reference.clone(),
                            },
                        }]
                    } else {
                        vec![]
                    },
                    appearance,
                ));
            }
        }

        let mut content = Flex::column().with_spacing(10.);
        content.add_child(
            Container::new(chips)
                .with_padding_left(12.)
                .with_padding_right(12.)
                .finish(),
        );
        content.add_child(self.render_skill_filter_bar(
            &scope_counts,
            total_skill_count,
            appearance,
        ));
        content.add_child(Self::render_tools_section(
            "Detected Skills",
            Some(format!(
                "{} skills from the {} scope",
                Self::metric_label("Showing".to_owned(), skill_rows.len()),
                self.skill_config_filter.label()
            )),
            skill_rows,
            appearance,
        ));
        content.add_child(Self::render_tools_section(
            "Provider Folders",
            Some(
                "Open or create skill folders for Warp, Claude, Codex, Gemini, OpenCode, and Agents"
                    .to_owned(),
            ),
            provider_rows,
            appearance,
        ));
        content.finish()
    }

    fn render_tools_config_panel(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let body = match self.tools_config_tab {
            ToolsConfigTab::Prompts => self.render_prompt_config_panel(app),
            ToolsConfigTab::Mcp => self.render_mcp_config_panel(app),
            ToolsConfigTab::Skills => self.render_skill_config_panel(app),
        };

        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(self.render_tools_config_header(app))
            .with_child(
                Shrinkable::new(
                    1.,
                    ClippedScrollable::vertical(
                        self.tools_config_scroll_state.clone(),
                        body,
                        ScrollbarWidth::Auto,
                        theme.nonactive_ui_detail().into(),
                        theme.active_ui_detail().into(),
                        ElementFill::None,
                    )
                    .with_overlayed_scrollbar()
                    .finish(),
                )
                .finish(),
            )
            .finish()
    }

    fn render_button(
        button_config: &ToolbeltButtonConfig,
        mouse_state: MouseStateHandle,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let action = button_config.action.clone();
        let ui_builder = appearance.ui_builder().clone();
        let tooltip_keybinding = button_config.tooltip_keybinding.clone();

        let icon_color = if button_config.render_with_active_state {
            appearance.theme().foreground().into_solid()
        } else {
            appearance
                .theme()
                .sub_text_color(appearance.theme().background())
                .into_solid()
        };

        let tooltip = if let Some(keybinding) = tooltip_keybinding {
            ui_builder
                .tool_tip_with_sublabel(button_config.tooltip_text.clone(), keybinding)
                .build()
                .finish()
        } else {
            ui_builder
                .tool_tip(button_config.tooltip_text.clone())
                .build()
                .finish()
        };

        let icon = if button_config.render_with_active_state {
            button_config.active_icon.unwrap_or(button_config.icon)
        } else {
            button_config.icon
        };

        icon_button(
            appearance,
            icon,
            button_config.render_with_active_state,
            mouse_state.clone(),
        )
        .with_tooltip(move || tooltip)
        .with_style(UiComponentStyles {
            font_color: Some(icon_color),
            height: Some(24.),
            width: Some(24.),
            padding: Some(Coords::uniform(4.)),
            ..Default::default()
        })
        .with_active_styles(UiComponentStyles {
            font_color: Some(icon_color),
            height: Some(24.),
            width: Some(24.),
            padding: Some(Coords::uniform(4.)),
            background: Some(internal_colors::fg_overlay_3(appearance.theme()).into()),
            ..Default::default()
        })
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }
}

impl LeftPanelView {
    pub fn handle_action_with_force_open(
        &mut self,
        action: &LeftPanelAction,
        force_open: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        match action {
            LeftPanelAction::ToolConfigurations => {
                active_view_state::set(self, ToolPanelView::ToolConfigurations, ctx);
            }
            LeftPanelAction::SelectToolsConfigTab(tab) => {
                self.tools_config_tab = *tab;
                active_view_state::set(self, ToolPanelView::ToolConfigurations, ctx);
            }
            LeftPanelAction::SelectToolsProviderFilter(filter) => {
                self.tools_provider_filter = *filter;
                active_view_state::set(self, ToolPanelView::ToolConfigurations, ctx);
            }
            LeftPanelAction::SelectSkillConfigFilter(filter) => {
                self.skill_config_filter = *filter;
                self.tools_config_tab = ToolsConfigTab::Skills;
                active_view_state::set(self, ToolPanelView::ToolConfigurations, ctx);
            }
            LeftPanelAction::ProjectExplorer => {
                active_view_state::set(self, ToolPanelView::ProjectExplorer, ctx);
                if force_open {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::FileTreeToggled {
                            source: FileTreeSource::ForceOpened,
                            is_code_mode_v2: true,
                            cli_agent: None,
                        },
                        ctx
                    );
                } else {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::FileTreeToggled {
                            source: FileTreeSource::LeftPanelToolbelt,
                            is_code_mode_v2: true,
                            cli_agent: None,
                        },
                        ctx
                    );
                }
            }
            LeftPanelAction::GlobalSearch { entry_focus } => {
                let was_active = self.active_view.get()
                    == ToolPanelView::GlobalSearch {
                        entry_focus: *entry_focus,
                    };
                active_view_state::set(
                    self,
                    ToolPanelView::GlobalSearch {
                        entry_focus: *entry_focus,
                    },
                    ctx,
                );
                if !was_active {
                    send_telemetry_from_ctx!(TelemetryEvent::GlobalSearchOpened, ctx);
                }
            }
            LeftPanelAction::WarpDrive => {
                active_view_state::set(self, ToolPanelView::WarpDrive, ctx);
                if force_open {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::WarpDriveOpened {
                            source: WarpDriveSource::ForceOpened,
                            is_code_mode_v2: true
                        },
                        ctx
                    );
                } else {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::WarpDriveOpened {
                            source: WarpDriveSource::LeftPanelToolbelt,
                            is_code_mode_v2: true
                        },
                        ctx
                    );
                }
            }
            LeftPanelAction::ConversationListView => {
                active_view_state::set(self, ToolPanelView::ConversationListView, ctx);
                send_telemetry_from_ctx!(TelemetryEvent::ConversationListViewOpened, ctx);
            }
            LeftPanelAction::SshRemote => {
                active_view_state::set(self, ToolPanelView::SshRemote, ctx);
            }
        }
    }

    pub fn on_left_panel_visibility_changed(&self, is_now_open: bool, ctx: &mut ViewContext<Self>) {
        if ToolPanelView::ConversationListView == self.active_view.get() {
            self.on_conversation_list_view_visibility_changed(is_now_open, ctx);
        }

        self.update_active_file_tree_subscription_state(ctx);
    }

    fn deactivate_file_tree_view_for_pane_group(
        &self,
        pane_group_id: warpui::EntityId,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_file_tree_view(pane_group_id)
        {
            view.update(ctx, |view, ctx| {
                view.set_is_active(false, ctx);
            });
        }
    }

    fn update_active_file_tree_subscription_state(&self, ctx: &mut ViewContext<Self>) {
        let Some(active_pane_group) = self
            .active_pane_group
            .as_ref()
            .and_then(|pane_group| pane_group.upgrade(ctx))
        else {
            return;
        };

        let is_visible = active_pane_group.as_ref(ctx).left_panel_open
            && self.active_view.get() == ToolPanelView::ProjectExplorer;

        if let Some(file_tree_view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_file_tree_view(active_pane_group.id())
        {
            file_tree_view.update(ctx, |view, ctx| {
                view.set_is_active(is_visible, ctx);
            });
        }
    }

    /// When the conversation list view's visibility changes,
    /// we need to update the conversation and tasks model to reflect the new state
    /// (this information is used to decide whether or not we should poll for new tasks).
    fn on_conversation_list_view_visibility_changed(
        &self,
        is_now_open: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let window_id = ctx.window_id();
        let view_id = self.conversation_list_view.id();
        AgentConversationsModel::handle(ctx).update(ctx, |model, ctx| {
            if is_now_open {
                model.register_view_open(window_id, view_id, ctx);
            } else {
                model.register_view_closed(window_id, view_id, ctx);
            }
        });
    }
}

impl TypedActionView for LeftPanelView {
    type Action = LeftPanelAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        self.handle_action_with_force_open(action, false, ctx);
    }
}

impl View for LeftPanelView {
    fn ui_name() -> &'static str {
        "LeftPanelView"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        // Focus the active tool panel view on-left-panel-focus.
        if focus_ctx.is_self_focused() {
            match self.active_view.get() {
                ToolPanelView::ToolConfigurations => {}
                ToolPanelView::ProjectExplorer => {
                    if let Some(view) = self.active_file_tree_view(ctx) {
                        ctx.focus(&view);
                    }
                }
                ToolPanelView::GlobalSearch { .. } => {
                    if let Some(view) = self.active_global_search_view(ctx) {
                        ctx.focus(&view);
                    }
                }
                ToolPanelView::WarpDrive => ctx.focus(&self.warp_drive_view),
                ToolPanelView::ConversationListView => ctx.focus(&self.conversation_list_view),
                ToolPanelView::SshRemote => ctx.focus(&self.ssh_remote_view),
            }
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);

        let mouse_state_handles = vec![
            self.mouse_state_handles.tools_configurations_button.clone(),
            self.mouse_state_handles.project_explorer_button.clone(),
            self.mouse_state_handles
                .conversation_list_view_button
                .clone(),
            self.mouse_state_handles.ssh_remote_button.clone(),
            self.mouse_state_handles.global_search_button.clone(),
            self.mouse_state_handles.warp_drive_button.clone(),
        ];

        // If there is only one button in the toolbelt row,
        // there is no need to show it as it's a bit redundant.
        let toolbelt_button_row = if self.toolbelt_buttons.len() > 1 {
            Some(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.0)
                    .with_children(self.toolbelt_buttons.iter().zip(&mouse_state_handles).map(
                        |(button_config, mouse_state)| {
                            Self::render_button(button_config, mouse_state.clone(), appearance)
                        },
                    ))
                    .with_main_axis_size(MainAxisSize::Min)
                    .finish(),
            )
        } else {
            None
        };

        let content_area: Box<dyn Element> = match self.active_view.get() {
            ToolPanelView::ToolConfigurations => {
                Shrinkable::new(1.0, self.render_tools_config_panel(app)).finish()
            }
            ToolPanelView::ProjectExplorer => {
                if let Some(file_tree_view) = self.active_file_tree_view(app) {
                    Shrinkable::new(
                        1.0,
                        Container::new(ChildView::new(&file_tree_view).finish())
                            .with_padding_left(2.)
                            .with_padding_right(2.)
                            .finish(),
                    )
                    .finish()
                } else {
                    Shrinkable::new(1.0, Container::new(Empty::new().finish()).finish()).finish()
                }
            }
            ToolPanelView::GlobalSearch { .. } => {
                if let Some(global_search_view) = self.active_global_search_view(app) {
                    Shrinkable::new(
                        1.0,
                        Container::new(ChildView::new(&global_search_view).finish()).finish(),
                    )
                    .finish()
                } else {
                    Shrinkable::new(1.0, Container::new(Empty::new().finish()).finish()).finish()
                }
            }
            ToolPanelView::WarpDrive => Shrinkable::new(
                1.0,
                Container::new(ChildView::new(&self.warp_drive_view).finish())
                    .with_padding_left(2.)
                    .with_padding_right(2.)
                    .finish(),
            )
            .finish(),
            ToolPanelView::ConversationListView => {
                Shrinkable::new(1.0, ChildView::new(&self.conversation_list_view).finish()).finish()
            }
            ToolPanelView::SshRemote => Shrinkable::new(
                1.0,
                SavePosition::new(
                    ChildView::new(&self.ssh_remote_view).finish(),
                    SSH_REMOTE_PANEL_POSITION_ID,
                )
                .finish(),
            )
            .finish(),
        };

        let panel_content = Container::new({
            let column = Flex::column();

            let header_left = if let Some(row) = toolbelt_button_row {
                row
            } else {
                Flex::row().finish()
            };

            let header_row = Container::new(
                ConstrainedBox::new(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(Shrinkable::new(1.0, header_left).finish())
                        .with_child(self.close_button(appearance, app))
                        .finish(),
                )
                .with_height(PANE_HEADER_HEIGHT)
                .finish(),
            )
            .with_padding_left(10.)
            .with_padding_right(HEADER_EDGE_PADDING)
            .finish();

            column
                .with_child(header_row)
                .with_child(Shrinkable::new(1.0, content_area).finish())
                .with_main_axis_size(MainAxisSize::Max)
                .finish()
        })
        .finish();

        if warpui::platform::is_mobile_device() {
            return panel_content;
        }

        let drag_side = match self.panel_position {
            super::PanelPosition::Left => DragBarSide::Right,
            super::PanelPosition::Right => DragBarSide::Left,
        };
        Resizable::new(self.resizable_state_handle.clone(), panel_content)
            .with_dragbar_side(drag_side)
            .on_resize(move |ctx, _| {
                ctx.notify();
            })
            .with_bounds_callback(Box::new(|window_size| {
                let min_width = MIN_SIDEBAR_WIDTH;
                let max_width = window_size.x() * MAX_SIDEBAR_WIDTH_RATIO;
                (min_width, max_width.max(min_width))
            }))
            .finish()
    }
}

fn deduplicate_by_directory_name(directories: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    directories
        .into_iter()
        .filter(|path| seen_paths.insert(path.clone()))
        .collect()
}
