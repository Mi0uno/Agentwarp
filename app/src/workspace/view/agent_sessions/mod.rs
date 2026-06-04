use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use chrono::Utc;
use pathfinder_geometry::vector::vec2f;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::ui::Icon;
use warp_core::user_preferences::GetUserPreferences as _;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    Border, ChildAnchor, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CornerRadius, CrossAxisAlignment, Element, Empty, Fill as ElementFill, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning, ParentAnchor,
    ParentElement, ParentOffsetBounds, Radius, ScrollbarWidth, Shrinkable, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::prelude::Align;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{
    AppContext, Entity, EntityId, ModelContext, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpEscapeKey, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::projects::ProjectManagementModel;
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionStatus, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::CLIAgent;
use crate::workspace::WorkspaceAction;

const AGENT_SESSION_RECORDS_PREF_KEY: &str = "agent_sessions.records.v1";
const MAX_AGENT_SESSION_RECORDS: usize = 200;
const MAX_TITLE_CHARS: usize = 96;
const MAX_HOSTED_TRANSCRIPT_CHARS: usize = 60_000;
pub(crate) const HOSTED_TRANSCRIPT_HEADER_PREFIX: &str = "--- Agentwarp saved chat history";
pub(crate) const HOSTED_TRANSCRIPT_END_MARKER: &str = "--- End saved chat history ---";
const AGENT_BUTTON_SIZE: f32 = 26.;
const SESSION_ACTION_BUTTON_SIZE: f32 = 20.;
const ICON_BUTTON_SIZE: f32 = 22.;
const SIDEBAR_HORIZONTAL_PADDING: f32 = 12.;

const SUPPORTED_AGENTS: [CLIAgent; 3] = [CLIAgent::Claude, CLIAgent::Codex, CLIAgent::OpenCode];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSessionStatus {
    Starting,
    InProgress,
    Success,
    Blocked,
    Unknown,
}

impl AgentSessionStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::InProgress => "Running",
            Self::Success => "Idle",
            Self::Blocked => "Waiting",
            Self::Unknown => "Unknown",
        }
    }

    fn from_cli_status(status: &CLIAgentSessionStatus) -> Self {
        match status {
            CLIAgentSessionStatus::InProgress => Self::InProgress,
            CLIAgentSessionStatus::Success => Self::Success,
            CLIAgentSessionStatus::Blocked { .. } => Self::Blocked,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub id: String,
    pub project_path: PathBuf,
    pub agent: CLIAgent,
    pub title: String,
    pub status: AgentSessionStatus,
    pub agent_session_id: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub archived_at_ms: Option<i64>,
    #[serde(default)]
    pub title_overridden: bool,
    #[serde(default)]
    pub hosted_transcript: Option<String>,
    #[serde(default)]
    pub hosted_transcript_updated_at_ms: Option<i64>,
    #[serde(skip, default)]
    pub terminal_view_id: Option<EntityId>,
}

impl AgentSessionRecord {
    fn is_archived(&self) -> bool {
        self.archived_at_ms.is_some()
    }

    pub fn hosted_transcript_for_restore(&self) -> Option<String> {
        self.hosted_transcript
            .as_deref()
            .map(str::trim)
            .filter(|transcript| !transcript.is_empty())
            .map(|transcript| {
                format!(
                    "\n{} ({}) ---\n{}\n{}\n\n",
                    HOSTED_TRANSCRIPT_HEADER_PREFIX,
                    self.agent.display_name(),
                    transcript.trim_end(),
                    HOSTED_TRANSCRIPT_END_MARKER
                )
            })
    }
}

#[derive(Debug, Clone)]
pub struct AgentSessionsModelEvent;

pub struct AgentSessionsModel {
    records: Vec<AgentSessionRecord>,
}

impl Entity for AgentSessionsModel {
    type Event = AgentSessionsModelEvent;
}

impl SingletonEntity for AgentSessionsModel {}

impl AgentSessionsModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), |me, event, ctx| {
            me.handle_cli_agent_sessions_event(event, ctx);
        });

        Self {
            records: read_records(ctx),
        }
    }

    pub fn records(&self) -> &[AgentSessionRecord] {
        &self.records
    }

    pub fn session(&self, session_id: &str) -> Option<&AgentSessionRecord> {
        self.records.iter().find(|record| record.id == session_id)
    }

    pub fn project_paths_from_sessions(&self) -> impl Iterator<Item = &PathBuf> {
        self.records.iter().map(|record| &record.project_path)
    }

    pub fn start_session(
        &mut self,
        project_path: PathBuf,
        agent: CLIAgent,
        ctx: &mut ModelContext<Self>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        self.records.insert(
            0,
            AgentSessionRecord {
                id: id.clone(),
                project_path,
                agent,
                title: new_session_title(agent),
                status: AgentSessionStatus::Starting,
                agent_session_id: None,
                updated_at_ms: now_ms(),
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
            },
        );
        self.trim_records();
        self.persist_and_emit(ctx);
        id
    }

    pub fn attach_terminal(
        &mut self,
        session_id: &str,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(record) = self.record_mut(session_id) else {
            return;
        };
        record.terminal_view_id = Some(terminal_view_id);
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    pub fn rename_session(
        &mut self,
        session_id: &str,
        title: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let title = truncate_title(title);
        if title.is_empty() {
            return;
        }

        let Some(record) = self.record_mut(session_id) else {
            return;
        };
        record.title = title;
        record.title_overridden = true;
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    pub fn toggle_pin(&mut self, session_id: &str, ctx: &mut ModelContext<Self>) {
        let Some(record) = self.record_mut(session_id) else {
            return;
        };
        record.is_pinned = !record.is_pinned;
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    pub fn toggle_archive(&mut self, session_id: &str, ctx: &mut ModelContext<Self>) {
        let Some(record) = self.record_mut(session_id) else {
            return;
        };
        record.archived_at_ms = if record.archived_at_ms.is_some() {
            None
        } else {
            Some(now_ms())
        };
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    pub fn delete_session(&mut self, session_id: &str, ctx: &mut ModelContext<Self>) {
        let original_len = self.records.len();
        self.records.retain(|record| record.id != session_id);
        if self.records.len() != original_len {
            self.persist_and_emit(ctx);
        }
    }

    pub fn update_hosted_transcript_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        transcript: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let transcript = normalize_hosted_transcript(transcript);
        let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.terminal_view_id == Some(terminal_view_id))
        else {
            return;
        };

        if record.hosted_transcript == transcript {
            return;
        }

        record.hosted_transcript = transcript;
        record.hosted_transcript_updated_at_ms = Some(now_ms());
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    fn handle_cli_agent_sessions_event(
        &mut self,
        event: &CLIAgentSessionsModelEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let terminal_view_id = event.terminal_view_id();
        let changed = match event {
            CLIAgentSessionsModelEvent::Started { agent, .. } => {
                self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    record.status = AgentSessionStatus::InProgress;
                })
            }
            CLIAgentSessionsModelEvent::StatusChanged {
                agent,
                status,
                session_context,
                ..
            } => self.update_record_for_terminal(terminal_view_id, |record| {
                record.agent = *agent;
                record.status = AgentSessionStatus::from_cli_status(status);
                if !record.title_overridden {
                    if let Some(title) = session_context.display_title() {
                        record.title = truncate_title(title);
                    }
                }
                if let Some(session_id) = &session_context.session_id {
                    record.agent_session_id = Some(session_id.clone());
                }
                record.capture_session_context(session_context);
            }),
            CLIAgentSessionsModelEvent::SessionUpdated { agent, .. } => {
                let session = CLIAgentSessionsModel::as_ref(ctx)
                    .session(terminal_view_id)
                    .cloned();
                self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    if let Some(session) = session {
                        record.status = AgentSessionStatus::from_cli_status(&session.status);
                        if !record.title_overridden {
                            if let Some(title) = session.session_context.display_title() {
                                record.title = truncate_title(title);
                            }
                        }
                        if let Some(session_id) = &session.session_context.session_id {
                            record.agent_session_id = Some(session_id.clone());
                        }
                        record.capture_session_context(&session.session_context);
                    }
                })
            }
            CLIAgentSessionsModelEvent::InputSessionChanged { .. } => false,
            CLIAgentSessionsModelEvent::Ended { agent, .. } => {
                self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    record.status = AgentSessionStatus::Success;
                })
            }
        };

        if changed {
            self.persist_and_emit(ctx);
        }
    }

    fn update_record_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        update: impl FnOnce(&mut AgentSessionRecord),
    ) -> bool {
        let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.terminal_view_id == Some(terminal_view_id))
        else {
            return false;
        };

        update(record);
        record.updated_at_ms = now_ms();
        true
    }

    fn record_mut(&mut self, session_id: &str) -> Option<&mut AgentSessionRecord> {
        self.records
            .iter_mut()
            .find(|record| record.id == session_id)
    }

    fn trim_records(&mut self) {
        if self.records.len() <= MAX_AGENT_SESSION_RECORDS {
            return;
        }

        self.records
            .sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        self.records.truncate(MAX_AGENT_SESSION_RECORDS);
    }

    fn persist_and_emit(&self, ctx: &mut ModelContext<Self>) {
        if let Ok(serialized) = serde_json::to_string(&self.records) {
            if let Err(err) = ctx
                .private_user_preferences()
                .write_value(AGENT_SESSION_RECORDS_PREF_KEY, serialized)
            {
                log::error!("Failed to persist agent session records: {err}");
            }
        }
        ctx.emit(AgentSessionsModelEvent);
    }
}

pub struct AgentSessionsView {
    scroll_state: ClippedScrollStateHandle,
    row_mouse_states: RefCell<HashMap<String, MouseStateHandle>>,
    rename_editor: ViewHandle<EditorView>,
    renaming_session_id: Option<String>,
    projects_header_mouse_state: MouseStateHandle,
    add_project_mouse_state: MouseStateHandle,
    empty_state_mouse_state: MouseStateHandle,
}

impl AgentSessionsView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&AgentSessionsModel::handle(ctx), |_me, _, _event, ctx| {
            ctx.notify();
        });
        ctx.subscribe_to_model(
            &ProjectManagementModel::handle(ctx),
            |_me, _, _event, ctx| {
                ctx.notify();
            },
        );

        let rename_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(12.), appearance),
                    select_all_on_focus: true,
                    clear_selections_on_blur: true,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    propagate_horizontal_navigation_keys: PropagateHorizontalNavigationKeys::Always,
                    propagate_and_no_op_escape_key: PropagateAndNoOpEscapeKey::HandleFirst,
                    max_buffer_len: Some(MAX_TITLE_CHARS),
                    ..Default::default()
                },
                ctx,
            )
        });
        ctx.subscribe_to_view(&rename_editor, |me, _handle, event, ctx| {
            me.handle_rename_editor_event(event, ctx);
        });

        Self {
            scroll_state: ClippedScrollStateHandle::default(),
            row_mouse_states: RefCell::new(HashMap::new()),
            rename_editor,
            renaming_session_id: None,
            projects_header_mouse_state: MouseStateHandle::default(),
            add_project_mouse_state: MouseStateHandle::default(),
            empty_state_mouse_state: MouseStateHandle::default(),
        }
    }

    fn handle_rename_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Enter | EditorEvent::Blurred => {
                self.commit_rename(ctx);
            }
            EditorEvent::Escape => {
                self.cancel_rename(ctx);
            }
            _ => {}
        }
    }

    fn begin_rename(&mut self, session_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(title) = AgentSessionsModel::as_ref(ctx)
            .session(session_id)
            .map(|record| record.title.clone())
        else {
            return;
        };

        self.renaming_session_id = Some(session_id.to_owned());
        self.rename_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(&title, ctx);
            editor.select_all(ctx);
        });
        ctx.focus(&self.rename_editor);
        ctx.notify();
    }

    fn commit_rename(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(session_id) = self.renaming_session_id.take() else {
            return;
        };
        let title = self.rename_editor.as_ref(ctx).buffer_text(ctx);
        AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
            model.rename_session(&session_id, title, ctx);
        });
        ctx.notify();
    }

    fn cancel_rename(&mut self, ctx: &mut ViewContext<Self>) {
        if self.renaming_session_id.take().is_some() {
            ctx.notify();
        }
    }

    fn project_paths(&self, app: &AppContext) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();
        for project in ProjectManagementModel::as_ref(app).all_projects() {
            paths.insert(PathBuf::from(project.path.clone()));
        }
        for path in AgentSessionsModel::as_ref(app).project_paths_from_sessions() {
            paths.insert(path.clone());
        }
        paths.into_iter().collect()
    }

    fn mouse_state(&self, key: impl Into<String>) -> MouseStateHandle {
        let key = key.into();
        self.row_mouse_states
            .borrow_mut()
            .entry(key)
            .or_default()
            .clone()
    }

    fn render_projects_header(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let header_mouse_state = self.projects_header_mouse_state.clone();
        let add_project_mouse_state = self.add_project_mouse_state.clone();

        Hoverable::new(header_mouse_state, move |state| {
            let action = WorkspaceAction::OpenAgentSessionProjectPicker;
            let trailing = if state.is_hovered() {
                render_icon_button(
                    add_project_mouse_state.clone(),
                    Icon::Plus,
                    "Add project",
                    appearance,
                    action,
                )
            } else {
                icon_button_placeholder()
            };

            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new_inline("Projects", appearance.ui_font_family(), 12.)
                            .with_color(theme.sub_text_color(theme.background()).into())
                            .with_style(Properties::default().weight(Weight::Medium))
                            .finish(),
                    )
                    .with_child(trailing)
                    .finish(),
            )
            .with_horizontal_padding(SIDEBAR_HORIZONTAL_PADDING)
            .with_vertical_padding(4.)
            .finish()
        })
        .with_defer_events_to_children()
        .finish()
    }

    fn render_project(
        &self,
        project_path: &Path,
        sessions: &[AgentSessionRecord],
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let font_family = appearance.ui_font_family();
        let project_name = project_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| project_path.to_string_lossy().to_string());
        let project_path_text = friendly_path(project_path);

        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(7.)
            .with_child(
                ConstrainedBox::new(
                    Icon::Folder
                        .to_warpui_icon(theme.sub_text_color(theme.background()))
                        .finish(),
                )
                .with_width(14.)
                .with_height(14.)
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.0,
                    Flex::column()
                        .with_spacing(1.)
                        .with_child(
                            Text::new_inline(project_name, font_family.clone(), 12.)
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .with_child(
                            Text::new_inline(project_path_text, font_family.clone(), 10.5)
                                .with_color(theme.sub_text_color(theme.background()).into())
                                .finish(),
                        )
                        .finish(),
                )
                .finish(),
            )
            .finish();

        let mut project_column = Flex::column().with_spacing(7.).with_child(header);

        let agent_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_children(
                SUPPORTED_AGENTS
                    .into_iter()
                    .map(|agent| self.render_agent_chip(project_path, agent, app)),
            )
            .finish();
        project_column.add_child(agent_row);

        let active_sessions = sessions
            .iter()
            .filter(|session| !session.is_archived())
            .collect::<Vec<_>>();
        let archived_sessions = sessions
            .iter()
            .filter(|session| session.is_archived())
            .collect::<Vec<_>>();

        if active_sessions.is_empty() && archived_sessions.is_empty() {
            project_column.add_child(
                Container::new(
                    Text::new_inline("No sessions", font_family, 12.)
                        .with_color(theme.disabled_ui_text_color().into())
                        .finish(),
                )
                .with_vertical_padding(6.)
                .finish(),
            );
        } else {
            for session in active_sessions {
                project_column.add_child(self.render_session_row(session, app));
            }
            if !archived_sessions.is_empty() {
                project_column.add_child(render_section_label("Archived", app));
                for session in archived_sessions {
                    project_column.add_child(self.render_session_row(session, app));
                }
            }
        }

        Container::new(project_column.finish())
            .with_horizontal_padding(SIDEBAR_HORIZONTAL_PADDING)
            .with_vertical_padding(6.)
            .finish()
    }

    fn render_agent_chip(
        &self,
        project_path: &Path,
        agent: CLIAgent,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let key = format!(
            "agent_chip:{}:{}",
            project_path.to_string_lossy(),
            agent.command_prefix()
        );
        let mouse_state = self.mouse_state(key);
        let project_path = project_path.to_path_buf();
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let tooltip_text = agent.display_name().to_string();
        let icon = agent.icon().unwrap_or(Icon::Terminal);
        let ui_builder = appearance.ui_builder().clone();

        Hoverable::new(mouse_state, move |state| {
            let icon_color = if state.is_hovered() {
                theme.main_text_color(theme.background())
            } else {
                theme.sub_text_color(theme.background())
            };
            let mut container = Container::new(
                Align::new(
                    ConstrainedBox::new(icon.to_warpui_icon(icon_color).finish())
                        .with_width(15.)
                        .with_height(15.)
                        .finish(),
                )
                .finish(),
            )
            .with_horizontal_padding(5.)
            .with_vertical_padding(5.)
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }

            let button = ConstrainedBox::new(container.finish())
                .with_width(AGENT_BUTTON_SIZE)
                .with_height(AGENT_BUTTON_SIZE)
                .finish();

            if state.is_hovered() {
                let tooltip = ui_builder.tool_tip(tooltip_text.clone()).build().finish();
                let mut stack = Stack::new().with_child(button);
                stack.add_positioned_overlay_child(
                    tooltip,
                    OffsetPositioning::offset_from_parent(
                        vec2f(0., 4.),
                        ParentOffsetBounds::WindowByPosition,
                        ParentAnchor::BottomMiddle,
                        ChildAnchor::TopMiddle,
                    ),
                );
                stack.finish()
            } else {
                button
            }
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(WorkspaceAction::StartAgentSession {
                project_path: project_path.clone(),
                agent,
            });
        })
        .finish()
    }

    fn render_session_row(
        &self,
        session: &AgentSessionRecord,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let font_family = appearance.ui_font_family();
        let mouse_state = self.mouse_state(format!("session:{}", session.id));
        let session_id = session.id.clone();
        let restore_session_id = session.id.clone();
        let title = session.title.clone();
        let is_pinned = session.is_pinned;
        let is_archived = session.is_archived();
        let is_renaming = self.renaming_session_id.as_deref() == Some(session.id.as_str());
        let meta = if is_archived {
            format!("{} - Archived", session.agent.display_name())
        } else {
            format!(
                "{} - {}",
                session.agent.display_name(),
                session.status.label()
            )
        };
        let status_fill = status_fill(session.status, app);
        let icon = session.agent.icon().unwrap_or(Icon::Terminal);
        let pin_button_state = self.mouse_state(format!("session_action:{}:pin", session.id));
        let rename_button_state = self.mouse_state(format!("session_action:{}:rename", session.id));
        let archive_button_state =
            self.mouse_state(format!("session_action:{}:archive", session.id));
        let delete_button_state = self.mouse_state(format!("session_action:{}:delete", session.id));
        let rename_editor = self.rename_editor.clone();

        let hoverable = Hoverable::new(mouse_state, move |state| {
            let title_element: Box<dyn Element> = if is_renaming {
                render_inline_rename_editor(&rename_editor, appearance, app)
            } else {
                let mut title_row = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.);
                if is_pinned {
                    title_row.add_child(
                        ConstrainedBox::new(
                            Icon::PinFilled
                                .to_warpui_icon(theme.sub_text_color(theme.background()))
                                .finish(),
                        )
                        .with_width(10.)
                        .with_height(10.)
                        .finish(),
                    );
                }
                title_row
                    .with_child(
                        Shrinkable::new(
                            1.0,
                            Text::new_inline(title.clone(), font_family.clone(), 12.)
                                .with_color(theme.main_text_color(theme.background()).into())
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish()
            };

            let actions = if state.is_hovered() && !is_renaming {
                render_session_actions(
                    &session_id,
                    is_pinned,
                    is_archived,
                    pin_button_state.clone(),
                    rename_button_state.clone(),
                    archive_button_state.clone(),
                    delete_button_state.clone(),
                    appearance,
                )
            } else {
                session_actions_placeholder()
            };

            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(7.)
                    .with_child(
                        ConstrainedBox::new(icon.to_warpui_icon(status_fill).finish())
                            .with_width(15.)
                            .with_height(15.)
                            .finish(),
                    )
                    .with_child(
                        Shrinkable::new(
                            1.0,
                            Flex::column()
                                .with_spacing(2.)
                                .with_child(title_element)
                                .with_child(
                                    Text::new_inline(meta.clone(), font_family.clone(), 11.)
                                        .with_color(theme.sub_text_color(theme.background()).into())
                                        .finish(),
                                )
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_child(actions)
                    .finish(),
            )
            .with_horizontal_padding(8.)
            .with_vertical_padding(7.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }
            container.finish()
        });

        let hoverable = if is_renaming {
            hoverable
        } else {
            hoverable
                .with_cursor(Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(WorkspaceAction::RestoreAgentSession {
                        session_id: restore_session_id.clone(),
                    });
                })
        };

        hoverable.with_defer_events_to_children().finish()
    }

    fn render_empty_state(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(12.)
            .with_child(
                ConstrainedBox::new(
                    Icon::Terminal
                        .to_warpui_icon(theme.sub_text_color(theme.background()))
                        .finish(),
                )
                .with_width(24.)
                .with_height(24.)
                .finish(),
            )
            .with_child(
                Text::new("No agent sessions", appearance.ui_font_family(), 14.)
                    .with_color(theme.sub_text_color(theme.background()).into_solid())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .with_child(render_compact_button(
                self.empty_state_mouse_state.clone(),
                Icon::Plus,
                "Project",
                app,
                WorkspaceAction::OpenAgentSessionProjectPicker,
            ))
            .finish();

        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(
                Flex::column()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(content)
                            .with_horizontal_padding(12.)
                            .finish(),
                    )
                    .finish(),
            )
            .finish()
    }
}

impl Entity for AgentSessionsView {
    type Event = ();
}

impl View for AgentSessionsView {
    fn ui_name() -> &'static str {
        "AgentSessionsView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let projects = self.project_paths(app);
        if projects.is_empty() {
            return self.render_empty_state(app);
        }

        let records = AgentSessionsModel::as_ref(app);
        let mut content = Flex::column()
            .with_spacing(1.)
            .with_child(self.render_projects_header(app));

        for project_path in projects {
            let mut sessions = records
                .records()
                .iter()
                .filter(|record| record.project_path == project_path)
                .cloned()
                .collect::<Vec<_>>();
            sort_sessions(&mut sessions);
            content.add_child(self.render_project(&project_path, &sessions, app));
        }

        ClippedScrollable::vertical(
            self.scroll_state.clone(),
            content.finish(),
            ScrollbarWidth::Auto,
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            ElementFill::None,
        )
        .with_overlayed_scrollbar()
        .finish()
    }
}

impl TypedActionView for AgentSessionsView {
    type Action = AgentSessionsViewAction;

    fn handle_action(&mut self, action: &AgentSessionsViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            AgentSessionsViewAction::TogglePin { session_id } => {
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.toggle_pin(session_id, ctx);
                });
            }
            AgentSessionsViewAction::BeginRename { session_id } => {
                self.begin_rename(session_id, ctx);
            }
            AgentSessionsViewAction::ToggleArchive { session_id } => {
                if self.renaming_session_id.as_deref() == Some(session_id.as_str()) {
                    self.cancel_rename(ctx);
                }
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.toggle_archive(session_id, ctx);
                });
            }
            AgentSessionsViewAction::Delete { session_id } => {
                if self.renaming_session_id.as_deref() == Some(session_id.as_str()) {
                    self.cancel_rename(ctx);
                }
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.delete_session(session_id, ctx);
                });
            }
        }
        ctx.notify();
    }
}

#[derive(Debug, Clone)]
pub enum AgentSessionsViewAction {
    TogglePin { session_id: String },
    BeginRename { session_id: String },
    ToggleArchive { session_id: String },
    Delete { session_id: String },
}

fn render_section_label(label: &'static str, app: &AppContext) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    Container::new(
        Text::new_inline(label, appearance.ui_font_family(), 11.)
            .with_color(theme.disabled_ui_text_color().into())
            .with_style(Properties::default().weight(Weight::Medium))
            .finish(),
    )
    .with_vertical_padding(4.)
    .with_horizontal_padding(2.)
    .finish()
}

fn sort_sessions(sessions: &mut [AgentSessionRecord]) {
    sessions.sort_by(|a, b| {
        b.is_pinned
            .cmp(&a.is_pinned)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
}

fn render_inline_rename_editor(
    rename_editor: &ViewHandle<EditorView>,
    appearance: &Appearance,
    app: &AppContext,
) -> Box<dyn Element> {
    let editor_line_height = rename_editor
        .as_ref(app)
        .line_height(app.font_cache(), appearance);
    TextInput::new(
        rename_editor.clone(),
        UiComponentStyles::default()
            .set_height(editor_line_height)
            .set_background(ElementFill::None)
            .set_border_radius(CornerRadius::with_all(Radius::Pixels(0.)))
            .set_border_width(0.),
    )
    .build()
    .finish()
}

fn session_actions_placeholder() -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_width(SESSION_ACTION_BUTTON_SIZE * 4. + 6.)
        .with_height(SESSION_ACTION_BUTTON_SIZE)
        .finish()
}

fn render_session_actions(
    session_id: &str,
    is_pinned: bool,
    is_archived: bool,
    pin_button_state: MouseStateHandle,
    rename_button_state: MouseStateHandle,
    archive_button_state: MouseStateHandle,
    delete_button_state: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.)
        .with_child(render_session_action_button(
            pin_button_state,
            if is_pinned {
                Icon::PinFilled
            } else {
                Icon::Pin
            },
            if is_pinned { "Unpin" } else { "Pin to top" },
            AgentSessionsViewAction::TogglePin {
                session_id: session_id.to_owned(),
            },
            is_pinned,
            false,
            appearance,
        ))
        .with_child(render_session_action_button(
            rename_button_state,
            Icon::Rename,
            "Rename",
            AgentSessionsViewAction::BeginRename {
                session_id: session_id.to_owned(),
            },
            false,
            false,
            appearance,
        ))
        .with_child(render_session_action_button(
            archive_button_state,
            Icon::Inbox,
            if is_archived { "Unarchive" } else { "Archive" },
            AgentSessionsViewAction::ToggleArchive {
                session_id: session_id.to_owned(),
            },
            is_archived,
            false,
            appearance,
        ))
        .with_child(render_session_action_button(
            delete_button_state,
            Icon::Trash,
            "Delete",
            AgentSessionsViewAction::Delete {
                session_id: session_id.to_owned(),
            },
            false,
            true,
            appearance,
        ))
        .finish()
}

fn render_session_action_button(
    mouse_state: MouseStateHandle,
    icon: Icon,
    tooltip_text: &'static str,
    action: AgentSessionsViewAction,
    is_selected: bool,
    is_danger: bool,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_builder = appearance.ui_builder().clone();

    Hoverable::new(mouse_state, move |state| {
        let icon_color = if is_danger && state.is_hovered() {
            ThemeFill::Solid(theme.ansi_fg_red())
        } else if is_selected || state.is_hovered() {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let mut button = Container::new(
            Align::new(
                ConstrainedBox::new(icon.to_warpui_icon(icon_color).finish())
                    .with_width(12.)
                    .with_height(12.)
                    .finish(),
            )
            .finish(),
        )
        .with_horizontal_padding(3.)
        .with_vertical_padding(3.)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if is_selected {
            button = button.with_background(theme.surface_overlay_1());
        }
        if state.is_hovered() {
            button = button.with_background(theme.surface_overlay_2());
        }

        let button = ConstrainedBox::new(button.finish())
            .with_width(SESSION_ACTION_BUTTON_SIZE)
            .with_height(SESSION_ACTION_BUTTON_SIZE)
            .finish();

        if state.is_hovered() {
            let tooltip = ui_builder
                .tool_tip(tooltip_text.to_string())
                .build()
                .finish();
            let mut stack = Stack::new().with_child(button);
            stack.add_positioned_overlay_child(
                tooltip,
                OffsetPositioning::offset_from_parent(
                    vec2f(0., 4.),
                    ParentOffsetBounds::WindowByPosition,
                    ParentAnchor::BottomMiddle,
                    ChildAnchor::TopMiddle,
                ),
            );
            stack.finish()
        } else {
            button
        }
    })
    .with_cursor(Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn icon_button_placeholder() -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_width(ICON_BUTTON_SIZE)
        .with_height(ICON_BUTTON_SIZE)
        .finish()
}

fn render_icon_button(
    mouse_state: MouseStateHandle,
    icon: Icon,
    tooltip_text: &'static str,
    appearance: &Appearance,
    action: WorkspaceAction,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_builder = appearance.ui_builder().clone();

    Hoverable::new(mouse_state, move |state| {
        let icon_color = if state.is_hovered() {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let mut button = Container::new(
            Align::new(
                ConstrainedBox::new(icon.to_warpui_icon(icon_color).finish())
                    .with_width(13.)
                    .with_height(13.)
                    .finish(),
            )
            .finish(),
        )
        .with_horizontal_padding(4.)
        .with_vertical_padding(4.)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if state.is_hovered() {
            button = button.with_background(theme.surface_overlay_1());
        }

        let button = ConstrainedBox::new(button.finish())
            .with_width(ICON_BUTTON_SIZE)
            .with_height(ICON_BUTTON_SIZE)
            .finish();

        if state.is_hovered() {
            let tooltip = ui_builder
                .tool_tip(tooltip_text.to_string())
                .build()
                .finish();
            let mut stack = Stack::new().with_child(button);
            stack.add_positioned_overlay_child(
                tooltip,
                OffsetPositioning::offset_from_parent(
                    vec2f(0., 4.),
                    ParentOffsetBounds::WindowByPosition,
                    ParentAnchor::BottomMiddle,
                    ChildAnchor::TopMiddle,
                ),
            );
            stack.finish()
        } else {
            button
        }
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_compact_button(
    mouse_state: MouseStateHandle,
    icon: Icon,
    label: &'static str,
    app: &AppContext,
    action: WorkspaceAction,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let mut container = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.)
                .with_child(
                    ConstrainedBox::new(
                        icon.to_warpui_icon(theme.main_text_color(theme.background()))
                            .finish(),
                    )
                    .with_width(13.)
                    .with_height(13.)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(label, appearance.ui_font_family(), 11.)
                        .with_color(theme.main_text_color(theme.background()).into())
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(8.)
        .with_vertical_padding(5.)
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
        if state.is_hovered() {
            container = container.with_background(theme.surface_overlay_1());
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn read_records(ctx: &ModelContext<AgentSessionsModel>) -> Vec<AgentSessionRecord> {
    ctx.private_user_preferences()
        .read_value(AGENT_SESSION_RECORDS_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| serde_json::from_str::<Vec<AgentSessionRecord>>(&serialized).ok())
        .unwrap_or_default()
        .into_iter()
        .take(MAX_AGENT_SESSION_RECORDS)
        .collect()
}

fn friendly_path(path: &Path) -> String {
    let raw_path = path.to_string_lossy();
    let home = dirs::home_dir().and_then(|path| path.to_str().map(str::to_owned));
    user_friendly_path(&raw_path, home.as_deref()).into_owned()
}

fn new_session_title(agent: CLIAgent) -> String {
    format!("New {} session", agent.display_name())
}

fn status_fill(status: AgentSessionStatus, app: &AppContext) -> ThemeFill {
    let theme = Appearance::as_ref(app).theme();
    match status {
        AgentSessionStatus::Starting | AgentSessionStatus::InProgress => {
            ThemeFill::Solid(theme.ansi_fg_magenta())
        }
        AgentSessionStatus::Success => ThemeFill::Solid(theme.ansi_fg_green()),
        AgentSessionStatus::Blocked => ThemeFill::Solid(theme.ansi_fg_yellow()),
        AgentSessionStatus::Unknown => theme.sub_text_color(theme.background()),
    }
}

fn truncate_title(title: String) -> String {
    let trimmed = title.trim();
    let mut chars = trimmed.chars();
    let truncated = chars.by_ref().take(MAX_TITLE_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn normalize_hosted_transcript(transcript: String) -> Option<String> {
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(tail_truncate_chars(trimmed, MAX_HOSTED_TRANSCRIPT_CHARS))
}

fn tail_truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut tail = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    tail.reverse();
    format!(
        "[Earlier saved history omitted]\n{}",
        tail.into_iter().collect::<String>()
    )
}

impl AgentSessionRecord {
    fn capture_session_context(
        &mut self,
        session_context: &crate::terminal::cli_agent_sessions::CLIAgentSessionContext,
    ) {
        let mut transcript = self.hosted_transcript.clone().unwrap_or_default();
        let original = transcript.clone();

        if let Some(query) = session_context.query.as_deref() {
            append_hosted_transcript_section(&mut transcript, "User", query);
        }
        if let Some(response) = session_context.response.as_deref() {
            append_hosted_transcript_section(&mut transcript, "Agent", response);
        }

        if transcript != original {
            self.hosted_transcript = normalize_hosted_transcript(transcript);
            self.hosted_transcript_updated_at_ms = Some(now_ms());
        }
    }
}

fn append_hosted_transcript_section(transcript: &mut String, label: &str, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    let section = format!("{label}:\n{text}\n\n");
    if transcript.trim_end().ends_with(section.trim_end()) {
        return;
    }
    if !transcript.trim().is_empty() && !transcript.ends_with('\n') {
        transcript.push('\n');
    }
    transcript.push_str(&section);
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_record_skips_terminal_view_id() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Codex,
            title: "Fix parser".to_owned(),
            status: AgentSessionStatus::InProgress,
            agent_session_id: Some("agent-session".to_owned()),
            updated_at_ms: 10,
            is_pinned: true,
            archived_at_ms: Some(11),
            title_overridden: true,
            hosted_transcript: Some("User:\nhello\n\nAgent:\nhi\n\n".to_owned()),
            hosted_transcript_updated_at_ms: Some(12),
            terminal_view_id: None,
        };

        let serialized = serde_json::to_string(&record).unwrap();
        assert!(!serialized.contains("terminal_view_id"));
        assert!(serialized.contains("hosted_transcript"));

        let restored = serde_json::from_str::<AgentSessionRecord>(&serialized).unwrap();
        assert_eq!(restored.id, "record-1");
        assert_eq!(restored.agent, CLIAgent::Codex);
        assert_eq!(restored.is_pinned, true);
        assert_eq!(restored.archived_at_ms, Some(11));
        assert_eq!(restored.title_overridden, true);
        assert_eq!(
            restored.hosted_transcript.as_deref(),
            Some("User:\nhello\n\nAgent:\nhi\n\n")
        );
        assert_eq!(restored.hosted_transcript_updated_at_ms, Some(12));
        assert_eq!(restored.terminal_view_id, None);
    }

    #[test]
    fn persisted_record_defaults_management_fields() {
        let restored = serde_json::from_str::<AgentSessionRecord>(
            r#"{
                "id": "record-1",
                "project_path": "/tmp/project",
                "agent": "Codex",
                "title": "Fix parser",
                "status": "InProgress",
                "agent_session_id": null,
                "updated_at_ms": 10
            }"#,
        )
        .unwrap();

        assert!(!restored.is_pinned);
        assert_eq!(restored.archived_at_ms, None);
        assert!(!restored.title_overridden);
        assert_eq!(restored.hosted_transcript, None);
        assert_eq!(restored.hosted_transcript_updated_at_ms, None);
    }

    #[test]
    fn hosted_transcript_is_trimmed_and_wrapped_for_restore() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Claude,
            title: "Fix parser".to_owned(),
            status: AgentSessionStatus::Success,
            agent_session_id: None,
            updated_at_ms: 10,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            hosted_transcript: normalize_hosted_transcript("  User:\nhello\n\n  ".to_owned()),
            hosted_transcript_updated_at_ms: Some(12),
            terminal_view_id: None,
        };

        let restore_text = record.hosted_transcript_for_restore().unwrap();
        assert!(restore_text.contains("Agentwarp saved chat history (Claude Code)"));
        assert!(restore_text.contains("User:\nhello"));
    }

    #[test]
    fn sort_sessions_pins_first_then_recent() {
        let mut records = vec![
            AgentSessionRecord {
                id: "old-pinned".to_owned(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "Old pinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                updated_at_ms: 1,
                is_pinned: true,
                archived_at_ms: None,
                title_overridden: false,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
            },
            AgentSessionRecord {
                id: "new-unpinned".to_owned(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "New unpinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                updated_at_ms: 20,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
            },
            AgentSessionRecord {
                id: "new-pinned".to_owned(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "New pinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                updated_at_ms: 10,
                is_pinned: true,
                archived_at_ms: None,
                title_overridden: false,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
            },
        ];

        sort_sessions(&mut records);

        assert_eq!(
            records
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec!["new-pinned", "old-pinned", "new-unpinned"]
        );
    }

    #[test]
    fn cli_status_maps_to_sidebar_status() {
        assert_eq!(
            AgentSessionStatus::from_cli_status(&CLIAgentSessionStatus::InProgress),
            AgentSessionStatus::InProgress
        );
        assert_eq!(
            AgentSessionStatus::from_cli_status(&CLIAgentSessionStatus::Success),
            AgentSessionStatus::Success
        );
        assert_eq!(
            AgentSessionStatus::from_cli_status(&CLIAgentSessionStatus::Blocked { message: None }),
            AgentSessionStatus::Blocked
        );
    }
}
