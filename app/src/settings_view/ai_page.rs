use ::ai::api_keys::{ApiKeyManager, ApiKeys};
use enum_iterator::all;
use itertools::Itertools;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use regex::Regex;
use settings::{Setting, ToggleableSetting};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use strum::IntoEnumIterator;
use warp_core::channel::ChannelState;
use warp_core::context_flag::ContextFlag;
use warp_core::features::FeatureFlag;
use warp_core::ui::color::contrast::MinimumAllowedContrast;
use warp_core::ui::color::ContrastingColor;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Align, Border, ChildAnchor, ChildView, ClippedScrollStateHandle, ClippedScrollable,
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Dismiss, Empty, Expanded,
    Fill as ElementFill, Fill, Flex, FormattedTextElement, HighlightedHyperlink, Hoverable,
    HyperlinkLens, HyperlinkUrl, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, ScrollbarWidth,
    Shrinkable, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::keymap::{ContextPredicate, Keystroke};
use warpui::platform::Cursor;
use warpui::r#async::Timer;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::slider::SliderStateHandle;
use warpui::ui_components::switch::{SwitchStateHandle, TooltipConfig};
use warpui::{
    id, Action, AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use super::custom_inference_modal::{
    CustomEndpointModal, CustomEndpointModalEvent, CustomEndpointModalViewState,
};
use super::execution_profile_view::{ExecutionProfileView, ExecutionProfileViewEvent};
use super::remove_custom_endpoint_confirmation_dialog::{
    RemoveCustomEndpointConfirmationDialog, RemoveCustomEndpointConfirmationDialogEvent,
};
use super::settings_page::{
    build_sub_header, build_toggle_element, render_body_item_label,
    render_body_item_label_with_icon, render_custom_size_header, render_dropdown_item,
    render_dropdown_item_label, render_full_pane_width_ai_button, render_input_list,
    render_separator, render_settings_info_banner, InputListItem, LocalOnlyIconState, MatchData,
    PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget, ToggleState,
    HEADER_PADDING, TOGGLE_BUTTON_RIGHT_PADDING,
};
use super::{
    flags, SettingActionPairContexts, SettingActionPairDescriptions, SettingsAction,
    SettingsSection, ToggleSettingActionPair,
};
#[cfg(not(target_family = "wasm"))]
use crate::ai::aws_credentials::refresh_aws_credentials;
use crate::ai::blocklist::agent_view::agent_input_footer::editor::{
    AgentToolbarEditorMode, AgentToolbarInlineEditor,
};
use crate::ai::blocklist::BlocklistAIPermissions;
use crate::ai::execution_profiles::model_menu_items::available_model_menu_items;
use crate::ai::execution_profiles::profiles::{
    AIExecutionProfilesModel, AIExecutionProfilesModelEvent, ClientProfileId,
};
use crate::ai::execution_profiles::{
    long_context_pricing_warning_title, AIExecutionProfile, AIExecutionProfileAppExt,
    ActionPermission, WriteToPtyPermission,
};
use crate::ai::llms::{LLMContextWindow, LLMId, LLMPreferences, LLMPreferencesEvent};
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::ai::paths::host_native_absolute_path;
use crate::auth::auth_manager::{AuthManager, LoginGatedFeature};
use crate::auth::auth_view_modal::AuthViewVariant;
use crate::auth::AuthStateProvider;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};
use crate::cloud_object::GenericStringObjectFormat::Json;
use crate::cloud_object::{JsonObjectType, ObjectType};
use crate::editor::{
    EditorOptions, EnterAction, EnterSettings, InteractionState, SingleLineEditorOptions,
    TextColors,
};
use crate::modal::{Modal, ModalEvent, ModalViewState};
use crate::settings::{
    AIAutoDetectionEnabled, AICommandDenylist, AISettingsChangedEvent,
    AgentModeCodingPermissionsType, AgentModeCommandExecutionDenylist,
    AgentModeCommandExecutionPredicate, AgentModeQuerySuggestionsEnabled, AwsBedrockAutoLogin,
    AwsBedrockCredentialsEnabled, CLIAgentApiModelMapping, CLIAgentApiProfile,
    CLIAgentApiProfileHealth, CLIAgentApiTakeoverEnabled, CLIAgentBuiltinPromptMode,
    CanUseWarpCreditsForFallback, CodeSettings, CodebaseContextEnabled, FileBasedMcpEnabled,
    GitOperationsAutogenEnabled, IncludeAgentCommandsInHistory, InputSettings,
    IntelligentAutosuggestionsEnabled, MemoryEnabled, NLDInTerminalEnabled,
    NaturalLanguageAutosuggestionsEnabled, PromptSubmissionMode, RuleSuggestionsEnabled,
    SharedBlockTitleGenerationEnabled, ShouldRenderCLIAgentToolbar,
    ShouldRenderUseAgentToolbarForUserCommands, ShouldShowOzUpdatesInZeroState, ShowAgentTips,
    ShowConversationHistory, ShowHintText, ThinkingDisplayMode, VoiceInputEnabled,
    WarpDriveContextEnabled, CLI_AGENT_API_ALL_ENVIRONMENTS_ID, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID,
};
use crate::terminal::cli_agent::{AgentRuntimeSettingsModel, DEFAULT_CLI_AGENT_MODEL_LABEL};
use crate::terminal::session_settings::{SessionSettings, SessionSettingsChangedEvent};
use crate::terminal::CLIAgent;
use crate::view_components::action_button::{ActionButton, ButtonSize, SecondaryTheme};
use crate::view_components::{
    render_warning_box, FilterableDropdown, SubmittableTextInput, SubmittableTextInputEvent,
    WarningBoxConfig,
};
use crate::workspaces::user_workspaces::UserWorkspacesEvent;

/// Identifies which subpage of the AI settings the user is viewing.
/// When `None`, the page shows all widgets (legacy/full view).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AISubpage {
    /// The main "WarpAgent" page: global AI toggle + Active AI + Input + Other sections.
    WarpAgent,
    /// Agent profiles and permissions.
    Profiles,
    /// Knowledge / Rules settings.
    Knowledge,
    /// Built-in prompt settings for supported third-party CLI agents.
    BuiltinPrompts,
    /// Third-party CLI agent settings.
    ThirdPartyCLIAgents,
    /// Unified API endpoint, model mapping, and failover settings for CLI agents.
    AgentApiProfiles,
    /// Local CLI agent resume identifiers.
    SessionIds,
}

impl AISubpage {
    pub fn from_section(section: SettingsSection) -> Option<Self> {
        match section {
            SettingsSection::WarpAgent => Some(Self::WarpAgent),
            SettingsSection::AgentProfiles => Some(Self::Profiles),
            SettingsSection::Knowledge => Some(Self::Knowledge),
            SettingsSection::AgentBuiltinPrompts => Some(Self::BuiltinPrompts),
            SettingsSection::ThirdPartyCLIAgents => Some(Self::ThirdPartyCLIAgents),
            SettingsSection::AgentApiProfiles => Some(Self::AgentApiProfiles),
            SettingsSection::AgentSessionIds => Some(Self::SessionIds),
            // AgentMCPServers renders the standalone MCPServers page, not an AI subpage.
            _ => None,
        }
    }
}
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::ops::Not;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use markdown_parser::{FormattedText, FormattedTextFragment, FormattedTextLine};

use crate::ai::{AIRequestUsageModel, AIRequestUsageModelEvent};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, TextOptions};
use crate::menu::{MenuItem, MenuItemFields};
use crate::server::telemetry::{
    AgentModeAutoDetectionSettingOrigin, AutonomySettingToggleSource,
    ToggleCodeSuggestionsSettingSource,
};
use crate::settings::{AISettings, VoiceInputToggleKey};
use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon;
use crate::util::bindings;
use crate::view_components::dropdown::DropdownAction;
use crate::view_components::{Dropdown, DropdownItem};
use crate::workspace::view::ssh_remote::{
    ssh_remote_environment_id, sync_remote_claude_agent_api_settings,
    sync_remote_codex_agent_api_settings, SshRemoteModel,
};
use crate::workspace::view::agent_sessions::AgentSessionsModel;
use crate::workspaces::workspace::{AdminEnablementSetting, CustomerType};
use crate::{
    report_error, report_if_error, send_telemetry_from_ctx, TelemetryEvent, UserWorkspaces,
};

const CONTENT_FONT_SIZE: f32 = 12.;
const PRIMARY_HEADER_FONT_SIZE: f32 = 24.;

const AI_SETTINGS_DROPDOWN_WIDTH: f32 = 250.;
const AI_SETTINGS_DROPDOWN_MAX_HEIGHT: f32 = 250.;
const CONTEXT_WINDOW_SLIDER_WIDTH: f32 = 220.;
const CONTEXT_WINDOW_INPUT_BOX_WIDTH: f32 = 120.;

const CLI_AGENT_API_CUSTOM_PRESET_ID: &str = "custom";
const CLI_AGENT_API_ANTHROPIC_BASE_URL_ENV_KEY: &str = "ANTHROPIC_BASE_URL";
const CLI_AGENT_API_OPENAI_BASE_URL_ENV_KEY: &str = "OPENAI_BASE_URL";
const CLI_AGENT_API_CLAUDE_SETTINGS_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
];
const CLI_AGENT_API_CODEX_SETTINGS_ENV_KEYS: &[&str] =
    &["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL"];
const CLI_AGENT_API_CODEX_MODEL_MIGRATION_TARGET: &str = "gpt-5.4";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CLIAgentApiProfilePreset {
    id: &'static str,
    label: &'static str,
    profile_name: &'static str,
    agent: CLIAgent,
    base_url: &'static str,
    model: &'static str,
    priority: u32,
}

const CLI_AGENT_API_PROFILE_PRESETS: &[CLIAgentApiProfilePreset] = &[];

const NEXT_COMMAND_DESCRIPTION: &str = "Let AI suggest the next command to run based on your command history, outputs, and common workflows.";
const PROMPT_SUGGESTIONS_DESCRIPTION: &str = "Let AI suggest natural language prompts, as inline banners in the input, based on recent commands and their outputs.";
const SUGGESTED_CODE_BANNERS_DESCRIPTION: &str = "Let AI suggest code diffs and queries as inline banners in the blocklist, based on recent commands and their outputs.";
const NATURAL_LANGUAGE_AUTOSUGGESTIONS: &str =
    "Let AI suggest natural language autosuggestions, based on recent commands and their outputs.";
const SHARED_BLOCK_TITLE_GENERATION_DESCRIPTION: &str =
    "Let AI generate a title for your shared block based on the command and output.";
const GIT_OPERATIONS_AUTOGEN_DESCRIPTION: &str =
    "Let AI generate commit messages and pull request titles and descriptions.";
const WISPR_FLOW_URL: &str = "https://wisprflow.ai/";
const CUSTOM_INFERENCE_LEARN_MORE_URL: &str =
    "https://docs.warp.dev/support-and-community/plans-and-billing/bring-your-own-api-key/";
const CUSTOM_INFERENCE_TERMS_URL: &str = "https://www.warp.dev/legal/terms-of-service";
const CUSTOM_INFERENCE_INFO_TOOLTIP_MAX_WIDTH: f32 = 320.;

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "AI",
            builder(SettingsAction::AI(AISettingsPageAction::ToggleGlobalAI)),
            context,
            flags::IS_ANY_AI_ENABLED,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "Active AI",
            builder(SettingsAction::AI(AISettingsPageAction::ToggleActiveAI)),
            &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
            flags::IS_ACTIVE_AI_ENABLED,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            if FeatureFlag::AgentView.is_enabled() {
                "terminal command autodetection in agent input"
            } else {
                "natural language detection"
            },
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleAIInputAutoDetection,
            )),
            &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
            flags::AI_INPUT_AUTODETECTION_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::AgentMode.is_enabled())],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "agent prompt autodetection in terminal input",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleNLDInTerminal,
            )),
            &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
            flags::NLD_IN_TERMINAL_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::AgentView.is_enabled())],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "Next Command",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleIntelligentAutosuggestions,
            )),
            &(context.clone() & id!(flags::IS_ACTIVE_AI_ENABLED)),
            flags::INTELLIGENT_AUTOSUGGESTIONS_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "prompt suggestions",
            builder(SettingsAction::AI(
                AISettingsPageAction::TogglePromptSuggestions,
            )),
            &(context.clone() & id!(flags::IS_ACTIVE_AI_ENABLED)),
            flags::PROMPT_SUGGESTIONS_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "code suggestions",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleCodeSuggestions,
            )),
            &(context.clone()
                & id!(flags::IS_ACTIVE_AI_ENABLED)
                & id!(flags::PROMPT_SUGGESTIONS_FLAG)),
            flags::CODE_SUGGESTIONS_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::custom(
            SettingActionPairDescriptions::new("Show agent tips", "Hide agent tips"),
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleShowAgentTips,
            )),
            SettingActionPairContexts::new(
                context.clone() & id!(flags::IS_ANY_AI_ENABLED) & !id!(flags::SHOW_AGENT_TIPS_FLAG),
                context.clone() & id!(flags::IS_ANY_AI_ENABLED) & id!(flags::SHOW_AGENT_TIPS_FLAG),
            ),
            None,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::AgentTips.is_enabled())],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::custom(
            SettingActionPairDescriptions::new(
                "Show Oz changelog in new agent conversation view",
                "Hide Oz changelog in new agent conversation view",
            ),
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleShowOzUpdatesInZeroState,
            )),
            SettingActionPairContexts::new(
                context.clone()
                    & id!(flags::IS_ANY_AI_ENABLED)
                    & !id!(flags::SHOW_OZ_UPDATES_IN_ZERO_STATE_FLAG),
                context.clone()
                    & id!(flags::IS_ANY_AI_ENABLED)
                    & id!(flags::SHOW_OZ_UPDATES_IN_ZERO_STATE_FLAG),
            ),
            None,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::AgentView.is_enabled())],
        app,
    );
    {
        use warpui::keymap::FixedBinding;

        use crate::settings::ThinkingDisplayMode;

        let ai_context = context.clone() & id!(flags::IS_ANY_AI_ENABLED);
        let mode_bindings: Vec<FixedBinding> = ThinkingDisplayMode::iter()
            .map(|mode| {
                let context_flag = match mode {
                    ThinkingDisplayMode::ShowAndCollapse => {
                        flags::THINKING_DISPLAY_SHOW_AND_COLLAPSE
                    }
                    ThinkingDisplayMode::AlwaysShow => flags::THINKING_DISPLAY_ALWAYS_SHOW,
                    ThinkingDisplayMode::NeverShow => flags::THINKING_DISPLAY_NEVER_SHOW,
                };
                FixedBinding::empty(
                    mode.command_palette_description(),
                    builder(SettingsAction::AI(
                        AISettingsPageAction::SetThinkingDisplayMode(mode),
                    )),
                    ai_context.clone() & !id!(context_flag),
                )
                .with_group(bindings::BindingGroup::WarpAi.as_str())
            })
            .collect();
        app.register_fixed_bindings(mode_bindings);
    }
    if FeatureFlag::QueueSlashCommand.is_enabled() {
        use warpui::keymap::FixedBinding;

        let ai_context = context.clone() & id!(flags::IS_ANY_AI_ENABLED);
        let mode_bindings: Vec<FixedBinding> = PromptSubmissionMode::iter()
            .map(|mode| {
                let context_flag = match mode {
                    PromptSubmissionMode::Interrupt => flags::PROMPT_SUBMISSION_INTERRUPT,
                    PromptSubmissionMode::Queue => flags::PROMPT_SUBMISSION_QUEUE,
                };
                FixedBinding::empty(
                    mode.command_palette_description(),
                    builder(SettingsAction::AI(
                        AISettingsPageAction::SetPromptSubmissionMode(mode),
                    )),
                    ai_context.clone() & !id!(context_flag),
                )
                .with_group(bindings::BindingGroup::WarpAi.as_str())
            })
            .collect();
        app.register_fixed_bindings(mode_bindings);
    }
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "natural language autosuggestions",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleNaturalLanguageAutosuggestions,
            )),
            &(context.clone() & id!(flags::IS_ACTIVE_AI_ENABLED)),
            flags::NATURAL_LANGUAGE_AUTOSUGGESTIONS_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::PredictAMQueries.is_enabled())],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "shared block title generation",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleSharedTitleGeneration,
            )),
            &(context.clone() & id!(flags::IS_ACTIVE_AI_ENABLED)),
            flags::SHARED_BLOCK_TITLE_GENERATION_FLAG,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| FeatureFlag::SharedBlockTitleGeneration.is_enabled())],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "commit and pull request generation",
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleGitOperationsAutogen,
            )),
            &(context.clone() & id!(flags::IS_ACTIVE_AI_ENABLED)),
            flags::GIT_OPERATIONS_AUTOGEN_FLAG,
        )
        .with_enabled(|| FeatureFlag::GitOperationsInCodeReview.is_enabled())
        .is_supported_on_current_platform(
            AISettings::as_ref(app)
                .git_operations_autogen_enabled_internal
                .is_supported_on_current_platform()
                && UserWorkspaces::as_ref(app).is_git_operations_ai_enabled(),
        )],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "voice input",
            builder(SettingsAction::AI(AISettingsPageAction::ToggleVoiceInput)),
            &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
            flags::IS_VOICE_INPUT_ENABLED,
        )
        .with_group(bindings::BindingGroup::WarpAi)
        .with_enabled(|| cfg!(feature = "voice_input"))],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::custom(
            SettingActionPairDescriptions::new(
                "Show \"Use Agent\" footer",
                "Hide \"Use Agent\" footer",
            ),
            builder(SettingsAction::AI(
                AISettingsPageAction::ToggleUseAgentToolbar,
            )),
            SettingActionPairContexts::new(
                context.clone()
                    & id!(flags::IS_ANY_AI_ENABLED)
                    & !id!(flags::USE_AGENT_FOOTER_FLAG),
                context.clone() & id!(flags::IS_ANY_AI_ENABLED) & id!(flags::USE_AGENT_FOOTER_FLAG),
            ),
            None,
        )
        .with_group(bindings::BindingGroup::WarpAi)],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![
            ToggleSettingActionPair::new(
                "include agent-executed commands in history",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleIncludeAgentCommandsInHistory,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::INCLUDE_AGENT_COMMANDS_IN_HISTORY_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi),
            ToggleSettingActionPair::new(
                "conversation history in tools panel",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleShowConversationHistory,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::SHOW_CONVERSATION_HISTORY,
            )
            .with_group(bindings::BindingGroup::WarpAi),
            ToggleSettingActionPair::new(
                "model picker in prompt",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleShowBaseModelPickerInPrompt,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::SHOW_BASE_MODEL_PICKER_IN_PROMPT_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi),
            ToggleSettingActionPair::new(
                "coding agent toolbar",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleCLIAgentToolbar,
                )),
                context,
                flags::CLI_AGENT_FOOTER_ENABLED,
            )
            .with_group(bindings::BindingGroup::WarpAi),
        ],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![
            ToggleSettingActionPair::new(
                "Rules",
                builder(SettingsAction::AI(AISettingsPageAction::ToggleRules)),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::AI_RULES_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::AIRules.is_enabled()),
            ToggleSettingActionPair::new(
                "Suggested Rules",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleRuleSuggestions,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::SUGGESTED_RULES_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| {
                FeatureFlag::AIRules.is_enabled() && FeatureFlag::SuggestedRules.is_enabled()
            }),
            ToggleSettingActionPair::new(
                "Warp Drive as agent context",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleWarpDriveContext,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::WARP_DRIVE_CONTEXT_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::AIRules.is_enabled()),
            ToggleSettingActionPair::new(
                "Auto-spawn servers from third-party agents",
                builder(SettingsAction::AI(AISettingsPageAction::ToggleFileBasedMcp)),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::FILE_BASED_MCP_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| {
                FeatureFlag::McpServer.is_enabled()
                    && FeatureFlag::FileBasedMcp.is_enabled()
                    && ContextFlag::ShowMCPServers.is_enabled()
            }),
        ],
        app,
    );
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![
            ToggleSettingActionPair::new(
                "Warp credit fallback",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleCanUseWarpCreditsForFallback,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::WARP_CREDIT_FALLBACK_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .is_supported_on_current_platform(
                UserWorkspaces::as_ref(app).is_byo_api_key_enabled(app)
                    || (FeatureFlag::CustomInferenceEndpoints.is_enabled()
                        && UserWorkspaces::as_ref(app).is_custom_inference_enabled(app)),
            ),
            ToggleSettingActionPair::new(
                "auto show or hide Rich Input based on agent status",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleAutoToggleRichInput,
                )),
                &(context.clone() & id!(flags::CLI_AGENT_FOOTER_ENABLED)),
                flags::AUTO_TOGGLE_RICH_INPUT_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::CLIAgentRichInput.is_enabled()),
            ToggleSettingActionPair::new(
                "auto open Rich Input when a coding agent session starts",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleAutoOpenRichInputOnCLIAgentStart,
                )),
                &(context.clone() & id!(flags::CLI_AGENT_FOOTER_ENABLED)),
                flags::AUTO_OPEN_RICH_INPUT_ON_CLI_AGENT_START_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::CLIAgentRichInput.is_enabled()),
            ToggleSettingActionPair::new(
                "auto dismiss Rich Input after prompt submission",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleAutoDismissRichInputAfterSubmit,
                )),
                &(context.clone() & id!(flags::CLI_AGENT_FOOTER_ENABLED)),
                flags::AUTO_DISMISS_RICH_INPUT_AFTER_SUBMIT_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::CLIAgentRichInput.is_enabled()),
        ],
        app,
    );
    if !FeatureFlag::FullSourceCodeEmbedding.is_enabled() {
        ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
            vec![ToggleSettingActionPair::new(
                "codebase index",
                builder(SettingsAction::AI(
                    AISettingsPageAction::ToggleCodebaseContext,
                )),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::IS_CODEBASE_INDEXING_ENABLED,
            )],
            app,
        );
    }
}

pub struct AISettingsPageView {
    page: PageType<Self>,
    active_subpage: Option<AISubpage>,
    voice_input_toggle_key_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    local_only_icon_tooltip_states: RefCell<HashMap<String, MouseStateHandle>>,
    autodetection_denylist_editor: ViewHandle<EditorView>,
    autonomy_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,

    code_read_autonomy_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,

    code_read_allowlist_editor: ViewHandle<SubmittableTextInput>,
    code_read_allowlist_mouse_state_handles: Vec<MouseStateHandle>,

    command_execution_allowlist_editor: ViewHandle<SubmittableTextInput>,
    command_execution_allowlist_mouse_state_handles: Vec<MouseStateHandle>,
    command_execution_denylist_editor: ViewHandle<SubmittableTextInput>,
    command_execution_denylist_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_footer_command_editor: ViewHandle<SubmittableTextInput>,
    cli_agent_footer_command_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_footer_command_agent_dropdowns: Vec<ViewHandle<Dropdown<AISettingsPageAction>>>,
    cli_agent_builtin_prompt_editors: Vec<ViewHandle<EditorView>>,
    cli_agent_builtin_prompt_mode_dropdowns: Vec<ViewHandle<Dropdown<AISettingsPageAction>>>,
    cli_agent_api_profile_preset_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    cli_agent_api_profile_selected_preset_id: String,
    cli_agent_api_takeover_toggle: SwitchStateHandle,
    cli_agent_api_profile_agent_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    cli_agent_api_profile_environment_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    cli_agent_api_profile_draft_agent: CLIAgent,
    cli_agent_api_profile_draft_environment_id: String,
    cli_agent_api_profile_editing_profile_id: Option<String>,
    cli_agent_api_profile_editor_open: bool,
    cli_agent_api_profile_modal_scroll_state: ClippedScrollStateHandle,
    cli_agent_api_profile_name_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_base_url_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_openai_base_url_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_api_format_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_auth_env_var_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_full_url_mode_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_api_key_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_model_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_model_catalog_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_model_mappings_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_priority_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_input_cost_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_output_cost_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_extra_env_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_fetched_models: Vec<String>,
    cli_agent_api_profile_selected_fetched_model: String,
    cli_agent_api_profile_fetch_models_error: Option<String>,
    cli_agent_api_profile_fetching_models: bool,
    cli_agent_api_profile_fetch_models_request_id: u64,
    cli_agent_api_profile_model_picker_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    cli_agent_api_profile_add_model_open: bool,
    cli_agent_api_profile_model_mapping_open: bool,
    cli_agent_api_profile_save_feedback: Option<String>,
    cli_agent_api_profile_save_feedback_is_error: bool,
    cli_agent_api_profile_save_feedback_generation: u64,
    cli_agent_api_profile_open_add_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_add_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_cancel_edit_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_fetch_models_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_toggle_add_model_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_add_manual_model_button: ViewHandle<ActionButton>,
    cli_agent_api_profile_add_selected_model_button: ViewHandle<ActionButton>,
    cli_agent_api_profiles_json_editor: ViewHandle<EditorView>,
    cli_agent_api_profile_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_api_profile_edit_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_api_profile_check_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_api_profile_toggle_mouse_state_handles: Vec<MouseStateHandle>,
    cli_agent_api_profile_remove_mouse_state_handles: Vec<MouseStateHandle>,
    agent_session_id_search_editor: ViewHandle<EditorView>,
    agent_session_id_editing_record_id: Option<String>,
    agent_session_id_agent_editor: ViewHandle<EditorView>,
    agent_session_id_project_editor: ViewHandle<EditorView>,
    agent_session_id_value_editor: ViewHandle<EditorView>,
    agent_session_id_title_editor: ViewHandle<EditorView>,
    agent_toolbar_inline_editor: ViewHandle<AgentToolbarInlineEditor>,
    cli_agent_toolbar_inline_editor: ViewHandle<AgentToolbarInlineEditor>,

    apply_code_diffs_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,
    read_files_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,
    execute_commands_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,
    write_to_pty_autonomy_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,
    mcp_permissions_dropdown_menu: ViewHandle<Dropdown<AISettingsPageAction>>,

    // Allowlisting directories (default profile)
    directory_allowlist_mouse_state_handles: Vec<MouseStateHandle>,
    directory_allowlist_editor: ViewHandle<SubmittableTextInput>,

    // Allowlisting commands (default profile)
    command_allowlist_mouse_state_handles: Vec<MouseStateHandle>,
    command_allowlist_editor: ViewHandle<SubmittableTextInput>,

    // Denylisting commands (default profile)
    command_denylist_mouse_state_handles: Vec<MouseStateHandle>,
    command_denylist_tooltip_mouse_state_handles: Vec<MouseStateHandle>,
    command_denylist_editor: ViewHandle<SubmittableTextInput>,

    mcp_allowlist_mouse_state_handles: Vec<MouseStateHandle>,
    mcp_allowlist_dropdown: ViewHandle<FilterableDropdown<AISettingsPageAction>>,

    mcp_denylist_mouse_state_handles: Vec<MouseStateHandle>,
    mcp_denylist_dropdown: ViewHandle<FilterableDropdown<AISettingsPageAction>>,

    base_model_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    coding_model_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,

    context_window_slider_state: SliderStateHandle,
    context_window_editor: ViewHandle<EditorView>,
    last_synced_context_window_editor_value: Option<u32>,
    dragged_context_window_value: Option<u32>,

    thinking_display_mode_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    default_prompt_submission_mode_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,
    #[cfg(feature = "local_fs")]
    conversation_layout_dropdown: ViewHandle<Dropdown<AISettingsPageAction>>,

    // Profile views
    profile_views: Vec<ViewHandle<ExecutionProfileView>>,
    add_profile_button: ViewHandle<ActionButton>,

    // Custom inference (custom endpoints)
    custom_endpoint_modal_state: CustomEndpointModalViewState,
    remove_custom_endpoint_confirmation_dialog: ViewHandle<RemoveCustomEndpointConfirmationDialog>,
    pending_remove_custom_endpoint_index: Option<usize>,
    custom_inference_add_button: ViewHandle<ActionButton>,
    custom_endpoint_edit_buttons: Vec<ViewHandle<ActionButton>>,
}

impl AISettingsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);

        let workspace = UserWorkspaces::handle(ctx);
        let ai_autonomy_settings = workspace.as_ref(ctx).ai_autonomy_settings();
        ctx.subscribe_to_model(&workspace, |me, workspace, event, ctx| {
            if let UserWorkspacesEvent::TeamsChanged = event {
                me.refresh_all_execution_profile_ui(ctx);
                me.reset_execution_profile_mouse_state_handles(ctx);

                let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
                let ai_autonomy_settings = workspace.as_ref(ctx).ai_autonomy_settings();

                Self::update_editor_interaction_state(
                    me.command_denylist_editor.as_ref(ctx).editor().clone(),
                    is_any_ai_enabled,
                    ctx,
                );

                Self::update_editor_interaction_state(
                    me.command_allowlist_editor.as_ref(ctx).editor().clone(),
                    is_any_ai_enabled
                        && !ai_autonomy_settings.has_override_for_execute_commands_allowlist(),
                    ctx,
                );

                Self::update_editor_interaction_state(
                    me.directory_allowlist_editor.as_ref(ctx).editor().clone(),
                    is_any_ai_enabled
                        && !ai_autonomy_settings.has_override_for_read_files_allowlist(),
                    ctx,
                );

                me.sync_custom_endpoint_buttons(ctx);
                ctx.notify();
            }
        });

        let voice_input_toggle_key_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            if !AISettings::as_ref(ctx).is_voice_input_enabled(ctx) {
                dropdown.set_disabled(ctx);
            }

            let values = VoiceInputToggleKey::all_possible_values();
            let current_value = AISettings::as_ref(ctx).voice_input_toggle_key.value();
            let selected_index = values
                .iter()
                .position(|val| val == current_value)
                .unwrap_or_else(|| {
                    log::warn!(
                        "Could not find current VoiceInputToggleKey value in dropdown option list"
                    );
                    0
                });

            dropdown.add_items(
                values
                    .into_iter()
                    .map(|val| {
                        DropdownItem::new(
                            val.display_name(),
                            AISettingsPageAction::SetVoiceInputToggleKey(val),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_index(selected_index, ctx);

            dropdown
        });

        let coding_model_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_max_height(AI_SETTINGS_DROPDOWN_MAX_HEIGHT, ctx);
            dropdown
        });
        Self::refresh_coding_model_menu(&coding_model_dropdown, ctx);

        let base_model_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_max_height(AI_SETTINGS_DROPDOWN_MAX_HEIGHT, ctx);

            dropdown
        });
        Self::refresh_base_model_menu(&base_model_dropdown, ctx);

        let initial_context_window_value = Self::initial_context_window_value(ctx);
        let clamped_initial = Self::configurable_context_window(ctx)
            .map(|cw| initial_context_window_value.clamp(cw.min, cw.max))
            .unwrap_or(initial_context_window_value);
        let context_window_slider_state = SliderStateHandle::default();

        let context_window_editor = ctx.add_typed_action_view(|ctx| {
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(Appearance::as_ref(ctx).ui_font_size()),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_buffer_text(&clamped_initial.to_string(), ctx);
            editor
        });
        ctx.subscribe_to_view(&context_window_editor, |me, _, event, ctx| {
            me.handle_context_window_editor_event(event, ctx);
        });
        let last_synced_context_window_editor_value = Some(clamped_initial);

        let thinking_display_mode_dropdown =
            OtherAIWidget::create_thinking_display_mode_dropdown(ctx);
        // Set initial selection based on current setting value.
        {
            let current_mode = AISettings::as_ref(ctx).thinking_display_mode;
            thinking_display_mode_dropdown.update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetThinkingDisplayMode(current_mode),
                    ctx,
                );
            });
        }

        let default_prompt_submission_mode_dropdown =
            OtherAIWidget::create_default_prompt_submission_mode_dropdown(ctx);
        {
            let current_mode = AISettings::as_ref(ctx).default_prompt_submission_mode;
            default_prompt_submission_mode_dropdown.update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetPromptSubmissionMode(current_mode),
                    ctx,
                );
            });
        }

        let autonomy_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown
        });
        Self::refresh_autonomy_dropdown_menu(&autonomy_dropdown_menu, ctx);

        let code_read_autonomy_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown
        });
        Self::refresh_code_read_autonomy_dropdown_menu(&code_read_autonomy_dropdown_menu, ctx);

        // While the data model supports arbitrary files in the allowlist,
        // it's most intuitive to allowlist whole directories.
        let code_read_allowlist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input = SubmittableTextInput::new(ctx).validate_on_submit(|s| {
                let expanded = host_native_absolute_path(s, &None, &None);
                Path::new(&expanded).is_dir()
            });
            input.set_placeholder_text("e.g. ~/code-repos/repo", ctx);
            input
        });
        Self::update_editor_interaction_state(
            code_read_allowlist_editor.as_ref(ctx).editor().clone(),
            is_any_ai_enabled,
            ctx,
        );

        ctx.subscribe_to_view(&code_read_allowlist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let expanded = host_native_absolute_path(s, &None, &None);
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(
                        model.add_filepath_to_code_read_allowlist(PathBuf::from(expanded), ctx)
                    );
                });
            }
        });

        let autodetection_denylist_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let options = EditorOptions {
                autogrow: true,
                soft_wrap: true,
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

            editor.set_placeholder_text("Commands, comma separated", ctx);

            let current_value = AISettings::as_ref(ctx)
                .autodetection_command_denylist
                .value()
                .clone();
            editor.set_buffer_text(current_value.as_str(), ctx);
            editor
        });
        Self::update_editor_interaction_state(
            autodetection_denylist_editor.clone(),
            is_any_ai_enabled,
            ctx,
        );

        ctx.subscribe_to_view(&autodetection_denylist_editor, move |me, _, event, ctx| {
            me.handle_detection_denylist_editor_event(event, ctx);
        });

        let command_execution_allowlist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input =
                SubmittableTextInput::new(ctx).validate_on_edit(|s| Regex::new(s).is_ok());
            input.set_placeholder_text("e.g. ls .*", ctx);
            input
        });
        Self::update_editor_interaction_state(
            command_execution_allowlist_editor
                .as_ref(ctx)
                .editor()
                .clone(),
            is_any_ai_enabled,
            ctx,
        );

        ctx.subscribe_to_view(&command_execution_allowlist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let predicate = match AgentModeCommandExecutionPredicate::new_regex(s) {
                    Ok(regex) => regex,
                    Err(e) => {
                        log::warn!(
                            "Failed to convert string to regex for cmd execution allowlist: {e}"
                        );
                        return;
                    }
                };
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(model.add_command_to_autoexecution_allowlist(predicate, ctx));
                })
            }
        });

        let command_execution_denylist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input =
                SubmittableTextInput::new(ctx).validate_on_edit(|s| Regex::new(s).is_ok());
            input.set_placeholder_text("e.g. rm .*", ctx);
            input
        });
        Self::update_editor_interaction_state(
            command_execution_denylist_editor
                .as_ref(ctx)
                .editor()
                .clone(),
            is_any_ai_enabled,
            ctx,
        );

        ctx.subscribe_to_view(&command_execution_denylist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let predicate = match AgentModeCommandExecutionPredicate::new_regex(s) {
                    Ok(regex) => regex,
                    Err(e) => {
                        log::warn!(
                            "Failed to convert string to regex for cmd execution denylist: {e}"
                        );
                        return;
                    }
                };
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(model.add_command_to_autoexecution_denylist(predicate, ctx));
                })
            }
        });

        let cli_agent_footer_command_editor = ctx.add_typed_action_view(|ctx| {
            let mut input =
                SubmittableTextInput::new(ctx).validate_on_edit(|s| Regex::new(s).is_ok());
            input.set_placeholder_text("command (supports regex)", ctx);
            input
        });
        // The coding agent footer command editor is always enabled,
        // independent of the global AI toggle, because it controls
        // third-party coding agents rather than Warp's own AI.
        Self::update_editor_interaction_state(
            cli_agent_footer_command_editor.as_ref(ctx).editor().clone(),
            true,
            ctx,
        );
        ctx.subscribe_to_view(
            &cli_agent_footer_command_editor,
            |_, _, event, ctx| match event {
                SubmittableTextInputEvent::Submit(command) => {
                    AISettings::handle(ctx).update(ctx, |settings, ctx| {
                        settings.add_cli_agent_footer_enabled_command(command, ctx);
                    });
                }
                SubmittableTextInputEvent::Escape => ctx.emit(AISettingsPageEvent::FocusModal),
            },
        );

        let cli_agent_builtin_prompt_editors = Self::create_cli_agent_builtin_prompt_editors(ctx);
        let cli_agent_builtin_prompt_mode_dropdowns =
            Self::create_cli_agent_builtin_prompt_mode_dropdowns(ctx);
        let cli_agent_api_profile_selected_preset_id = CLI_AGENT_API_CUSTOM_PRESET_ID.to_owned();
        let initial_cli_agent_api_profile = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .into_iter()
            .next();
        let cli_agent_api_profile_preset_dropdown =
            Self::create_cli_agent_api_profile_preset_dropdown(
                &cli_agent_api_profile_selected_preset_id,
                ctx,
            );
        let cli_agent_api_profile_draft_agent = initial_cli_agent_api_profile
            .as_ref()
            .map(CLIAgentApiProfile::agent)
            .unwrap_or(CLIAgent::Claude);
        let cli_agent_api_profile_draft_environment_id = initial_cli_agent_api_profile
            .as_ref()
            .map(|profile| profile.environment_id.clone())
            .unwrap_or_else(|| CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned());
        let cli_agent_api_profile_agent_dropdown =
            Self::create_cli_agent_api_profile_agent_dropdown(
                cli_agent_api_profile_draft_agent,
                ctx,
            );
        let cli_agent_api_profile_environment_dropdown =
            Self::create_cli_agent_api_profile_environment_dropdown(
                &cli_agent_api_profile_draft_environment_id,
                ctx,
            );
        let cli_agent_api_profile_name_editor =
            Self::create_cli_agent_api_profile_editor("Profile name", false, ctx);
        let cli_agent_api_profile_base_url_editor = Self::create_cli_agent_api_profile_editor(
            "https://api.example.com/anthropic",
            false,
            ctx,
        );
        let cli_agent_api_profile_openai_base_url_editor =
            Self::create_cli_agent_api_profile_editor("https://api.example.com", false, ctx);
        let cli_agent_api_profile_api_format_editor = Self::create_cli_agent_api_profile_editor(
            "anthropic_messages / openai_chat / gemini",
            false,
            ctx,
        );
        let cli_agent_api_profile_auth_env_var_editor =
            Self::create_cli_agent_api_profile_editor("ANTHROPIC_AUTH_TOKEN", false, ctx);
        let cli_agent_api_profile_full_url_mode_editor =
            Self::create_cli_agent_api_profile_editor("false", false, ctx);
        let cli_agent_api_profile_api_key_editor =
            Self::create_cli_agent_api_profile_editor("API key", true, ctx);
        let cli_agent_api_profile_model_editor =
            Self::create_cli_agent_api_profile_editor("Optional model", false, ctx);
        let cli_agent_api_profile_model_catalog_editor =
            Self::create_cli_agent_api_profile_multiline_editor(
                "One model id per line, e.g. claude-3-5-sonnet-latest",
                ctx,
            );
        let cli_agent_api_profile_model_mappings_editor =
            Self::create_cli_agent_api_profile_multiline_editor(
                "Role, display, request model, 1M flag; e.g. Sonnet=qwen3.5-plus",
                ctx,
            );
        let cli_agent_api_profile_priority_editor =
            Self::create_cli_agent_api_profile_editor("0", false, ctx);
        let cli_agent_api_profile_input_cost_editor =
            Self::create_cli_agent_api_profile_editor("0.00", false, ctx);
        let cli_agent_api_profile_output_cost_editor =
            Self::create_cli_agent_api_profile_editor("0.00", false, ctx);
        let cli_agent_api_profile_extra_env_editor =
            Self::create_cli_agent_api_profile_extra_env_editor(ctx);
        if let Some(profile) = initial_cli_agent_api_profile.as_ref() {
            let anthropic_base_url = Self::cli_agent_api_profile_anthropic_base_url(profile);
            let openai_base_url = Self::cli_agent_api_profile_openai_base_url(profile);
            for (editor, text) in [
                (&cli_agent_api_profile_name_editor, profile.name.clone()),
                (&cli_agent_api_profile_base_url_editor, anthropic_base_url),
                (
                    &cli_agent_api_profile_openai_base_url_editor,
                    openai_base_url,
                ),
                (
                    &cli_agent_api_profile_api_format_editor,
                    profile.api_format.clone(),
                ),
                (
                    &cli_agent_api_profile_auth_env_var_editor,
                    profile.auth_env_var.clone(),
                ),
                (
                    &cli_agent_api_profile_full_url_mode_editor,
                    profile.full_url_mode.to_string(),
                ),
                (
                    &cli_agent_api_profile_api_key_editor,
                    profile.api_key.clone(),
                ),
                (&cli_agent_api_profile_model_editor, profile.model.clone()),
                (
                    &cli_agent_api_profile_model_catalog_editor,
                    Self::format_cli_agent_api_model_catalog(&profile.model_catalog),
                ),
                (
                    &cli_agent_api_profile_model_mappings_editor,
                    Self::format_cli_agent_api_model_mappings(&profile.model_mappings),
                ),
                (
                    &cli_agent_api_profile_priority_editor,
                    profile.priority.to_string(),
                ),
                (
                    &cli_agent_api_profile_input_cost_editor,
                    profile.input_cost_per_million_tokens.to_string(),
                ),
                (
                    &cli_agent_api_profile_output_cost_editor,
                    profile.output_cost_per_million_tokens.to_string(),
                ),
                (
                    &cli_agent_api_profile_extra_env_editor,
                    Self::format_cli_agent_api_extra_env(&profile.extra_env),
                ),
            ] {
                editor.update(ctx, |editor, ctx| editor.set_buffer_text(&text, ctx));
            }
        }
        let cli_agent_api_profile_open_add_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("添加供应商", SecondaryTheme)
                .with_icon(Icon::Plus)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::OpenAddCLIAgentApiProfileModal);
                })
        });
        let cli_agent_api_profile_add_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("保存", SecondaryTheme)
                .with_icon(Icon::Check)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::AddCLIAgentApiProfile);
                })
        });
        let cli_agent_api_profile_cancel_edit_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", SecondaryTheme)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::CancelEditCLIAgentApiProfile);
                })
        });
        let cli_agent_api_profile_fetch_models_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("获取模型", SecondaryTheme)
                .with_icon(Icon::RefreshCw04)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::FetchCLIAgentApiProfileModels);
                })
        });
        let cli_agent_api_profile_toggle_add_model_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("添加模型", SecondaryTheme)
                .with_icon(Icon::Plus)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        AISettingsPageAction::ToggleCLIAgentApiProfileAddModel,
                    );
                })
        });
        let cli_agent_api_profile_add_manual_model_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("添加", SecondaryTheme)
                .with_icon(Icon::Plus)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        AISettingsPageAction::AddCLIAgentApiProfileDraftModel,
                    );
                })
        });
        let cli_agent_api_profile_add_selected_model_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("添加所选", SecondaryTheme)
                .with_icon(Icon::Check)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        AISettingsPageAction::AddCLIAgentApiProfileSelectedFetchedModel,
                    );
                })
        });
        let initial_cli_agent_api_fetched_models = initial_cli_agent_api_profile
            .as_ref()
            .map(|profile| profile.model_catalog.clone())
            .unwrap_or_default();
        let initial_cli_agent_api_selected_fetched_model = initial_cli_agent_api_profile
            .as_ref()
            .map(|profile| profile.preferred_model())
            .filter(|model| !model.trim().is_empty())
            .or_else(|| initial_cli_agent_api_fetched_models.first().cloned())
            .unwrap_or_default();
        let cli_agent_api_profile_model_picker_dropdown =
            Self::create_cli_agent_api_profile_model_picker_dropdown(
                &initial_cli_agent_api_selected_fetched_model,
                &initial_cli_agent_api_fetched_models,
                ctx,
            );
        let cli_agent_api_profiles_json_editor =
            Self::create_cli_agent_api_profiles_json_editor(ctx);
        let agent_session_id_search_editor =
            Self::create_cli_agent_api_profile_editor("Search sessions", false, ctx);
        ctx.subscribe_to_view(&agent_session_id_search_editor, |_, _, _, ctx| {
            ctx.notify();
        });
        let agent_session_id_agent_editor =
            Self::create_cli_agent_api_profile_editor("codex", false, ctx);
        let agent_session_id_project_editor =
            Self::create_cli_agent_api_profile_editor("/path/to/project", false, ctx);
        let agent_session_id_value_editor =
            Self::create_cli_agent_api_profile_editor("session id", false, ctx);
        let agent_session_id_title_editor =
            Self::create_cli_agent_api_profile_editor("Title", false, ctx);
        let cli_agent_api_profile_count = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .len();
        let cli_agent_api_profile_mouse_state_handles = (0..cli_agent_api_profile_count)
            .map(|_| Default::default())
            .collect();
        let cli_agent_api_profile_edit_mouse_state_handles = (0..cli_agent_api_profile_count)
            .map(|_| Default::default())
            .collect();
        let cli_agent_api_profile_check_mouse_state_handles = (0..cli_agent_api_profile_count)
            .map(|_| Default::default())
            .collect();
        let cli_agent_api_profile_toggle_mouse_state_handles = (0..cli_agent_api_profile_count)
            .map(|_| Default::default())
            .collect();
        let cli_agent_api_profile_remove_mouse_state_handles = (0..cli_agent_api_profile_count)
            .map(|_| Default::default())
            .collect();

        ctx.subscribe_to_model(&SshRemoteModel::handle(ctx), |me, _, _, ctx| {
            me.refresh_cli_agent_api_profile_environment_dropdown(ctx);
            ctx.notify();
        });

        let request_usage_model = AIRequestUsageModel::handle(ctx);
        ctx.subscribe_to_model(&request_usage_model, |_, _, _, ctx| {
            // The only event is RequestUsageUpdated
            ctx.notify();
        });

        ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _handle, _event, ctx| {
            // Re-render if teams-related data changed that may affect whether features such as voice input are enabled.
            Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
            Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
            me.sync_custom_endpoint_buttons(ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &AIExecutionProfilesModel::handle(ctx),
            |me, _, event, ctx| {
                match event {
                    AIExecutionProfilesModelEvent::ProfileCreated
                    | AIExecutionProfilesModelEvent::ProfileDeleted => {
                        me.refresh_profile_views(ctx);
                    }
                    AIExecutionProfilesModelEvent::ProfileUpdated(_) => {
                        me.refresh_all_execution_profile_ui(ctx);
                        me.reset_execution_profile_mouse_state_handles(ctx);
                        me.sync_context_window_editor(ctx, false);
                    }
                    AIExecutionProfilesModelEvent::UpdatedActiveProfile { .. } => (),
                }
                ctx.notify();
            },
        );

        let cloud_model = CloudModel::handle(ctx);
        ctx.subscribe_to_model(&cloud_model, |me, _, event, ctx| {
            let added_or_deleted_mcp_servers = matches!(
                event,
                CloudModelEvent::ObjectCreated { type_and_id } | CloudModelEvent::ObjectDeleted { type_and_id, .. }
                if matches!(
                    type_and_id.object_type(),
                    ObjectType::GenericStringObject(Json(JsonObjectType::MCPServer))
                )
            );

            if added_or_deleted_mcp_servers {
                Self::refresh_mcp_allowlist_dropdown(&me.mcp_allowlist_dropdown, ctx);
                Self::refresh_mcp_denylist_dropdown(&me.mcp_denylist_dropdown, ctx);
                ctx.notify();
            }
        });

        let templatable_manager = TemplatableMCPServerManager::handle(ctx);
        ctx.subscribe_to_model(&templatable_manager, |me, _, _event, ctx| {
            Self::refresh_mcp_allowlist_dropdown(&me.mcp_allowlist_dropdown, ctx);
            Self::refresh_mcp_denylist_dropdown(&me.mcp_denylist_dropdown, ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &LLMPreferences::handle(ctx),
            |me, _, event, ctx| match event {
                LLMPreferencesEvent::UpdatedAvailableLLMs => {
                    Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
                    Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
                    me.sync_context_window_editor(ctx, false);
                }
                LLMPreferencesEvent::UpdatedActiveAgentModeLLM => {
                    Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
                    me.sync_context_window_editor(ctx, false);
                }
                LLMPreferencesEvent::UpdatedActiveCodingLLM => {
                    Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
                }
            },
        );

        // Refresh model dropdowns when BYO API keys update so key icons reflect latest state.
        ctx.subscribe_to_model(&ApiKeyManager::handle(ctx), |me, _model, _event, ctx| {
            Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
            Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
            me.sync_context_window_editor(ctx, false);
            me.sync_custom_endpoint_buttons(ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
            match event {
                AISettingsChangedEvent::AICommandDenylist { .. } => {
                    me.autodetection_denylist_editor.update(ctx, |editor, ctx| {
                        let denylist_value = &AISettings::as_ref(ctx)
                            .autodetection_command_denylist
                            .value()
                            .clone();
                        editor.set_buffer_text(denylist_value, ctx);
                    });
                }
                AISettingsChangedEvent::IsAnyAIEnabled { .. } => {
                    let is_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
                    let ai_autonomy_settings = UserWorkspaces::as_ref(ctx).ai_autonomy_settings();

                    Self::update_editor_interaction_state(
                        me.autodetection_denylist_editor.clone(),
                        is_enabled,
                        ctx,
                    );
                    Self::update_editor_interaction_state(
                        me.command_execution_allowlist_editor
                            .as_ref(ctx)
                            .editor()
                            .clone(),
                        is_enabled,
                        ctx,
                    );
                    Self::update_editor_interaction_state(
                        me.command_execution_denylist_editor
                            .as_ref(ctx)
                            .editor()
                            .clone(),
                        is_enabled,
                        ctx,
                    );
                    Self::update_editor_interaction_state(
                        me.code_read_allowlist_editor.as_ref(ctx).editor().clone(),
                        is_enabled,
                        ctx,
                    );

                    Self::update_editor_interaction_state(
                        me.directory_allowlist_editor.as_ref(ctx).editor().clone(),
                        is_enabled && !ai_autonomy_settings.has_override_for_read_files_allowlist(),
                        ctx,
                    );

                    Self::update_editor_interaction_state(
                        me.command_denylist_editor.as_ref(ctx).editor().clone(),
                        is_enabled,
                        ctx,
                    );

                    Self::update_editor_interaction_state(
                        me.command_allowlist_editor.as_ref(ctx).editor().clone(),
                        is_enabled
                            && !ai_autonomy_settings.has_override_for_execute_commands_allowlist(),
                        ctx,
                    );

                    me.update_voice_input_dropdown_enablement(ctx);
                    Self::refresh_autonomy_dropdown_menu(&me.autonomy_dropdown_menu, ctx);

                    me.refresh_all_execution_profile_ui(ctx);

                    Self::refresh_code_read_autonomy_dropdown_menu(
                        &me.code_read_autonomy_dropdown_menu,
                        ctx,
                    );
                    Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
                    Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
                    Self::refresh_mcp_allowlist_dropdown(&me.mcp_allowlist_dropdown, ctx);
                    Self::refresh_mcp_denylist_dropdown(&me.mcp_denylist_dropdown, ctx);
                    me.sync_context_window_editor(ctx, true);
                    me.sync_custom_endpoint_buttons(ctx);
                }
                AISettingsChangedEvent::VoiceInputEnabled { .. } => {
                    me.update_voice_input_dropdown_enablement(ctx);
                }
                AISettingsChangedEvent::AgentModeExecuteReadonlyCommands { .. } => {
                    Self::refresh_autonomy_dropdown_menu(&me.autonomy_dropdown_menu, ctx);
                    Self::refresh_code_read_autonomy_dropdown_menu(
                        &me.code_read_autonomy_dropdown_menu,
                        ctx,
                    );
                }
                AISettingsChangedEvent::AgentModeCodingPermissions { .. } => {
                    Self::refresh_code_read_autonomy_dropdown_menu(
                        &me.code_read_autonomy_dropdown_menu,
                        ctx,
                    );
                }
                AISettingsChangedEvent::VoiceInputToggleKey { .. } => {
                    let current_value = AISettings::as_ref(ctx)
                        .voice_input_toggle_key
                        .value()
                        .display_name();
                    me.voice_input_toggle_key_dropdown
                        .update(ctx, |dropdown, ctx| {
                            dropdown.set_selected_by_name(current_value, ctx)
                        });
                }
                AISettingsChangedEvent::AgentModeCommandExecutionAllowlist { .. } => {
                    me.command_execution_allowlist_mouse_state_handles = AISettings::as_ref(ctx)
                        .agent_mode_command_execution_allowlist
                        .value()
                        .iter()
                        .map(|_| Default::default())
                        .collect();
                }
                AISettingsChangedEvent::AgentModeCommandExecutionDenylist { .. } => {
                    me.command_execution_denylist_mouse_state_handles = AISettings::as_ref(ctx)
                        .agent_mode_command_execution_denylist
                        .value()
                        .iter()
                        .map(|_| Default::default())
                        .collect();
                }
                AISettingsChangedEvent::AgentModeCodingFileReadAllowlist { .. } => {
                    me.code_read_allowlist_mouse_state_handles = AISettings::as_ref(ctx)
                        .agent_mode_coding_file_read_allowlist
                        .value()
                        .iter()
                        .map(|_| Default::default())
                        .collect();
                }
                AISettingsChangedEvent::CLIAgentToolbarEnabledCommands { .. } => {
                    me.cli_agent_footer_command_mouse_state_handles = AISettings::as_ref(ctx)
                        .cli_agent_footer_enabled_commands
                        .value()
                        .keys()
                        .map(|_| Default::default())
                        .collect();
                    me.cli_agent_footer_command_agent_dropdowns =
                        Self::create_cli_agent_dropdowns(ctx);
                }
                AISettingsChangedEvent::CLIAgentBuiltinPrompts { .. } => {
                    me.sync_cli_agent_builtin_prompt_editors(ctx);
                    me.refresh_cli_agent_builtin_prompt_mode_dropdowns(ctx);
                }
                AISettingsChangedEvent::CLIAgentApiProfiles { .. } => {
                    me.sync_cli_agent_api_profile_mouse_state_handles(ctx);
                }
                AISettingsChangedEvent::ThinkingDisplayMode { .. } => {
                    let current_mode = *AISettings::as_ref(ctx).thinking_display_mode.value();
                    me.thinking_display_mode_dropdown
                        .update(ctx, |dropdown, ctx| {
                            dropdown.set_selected_by_action(
                                AISettingsPageAction::SetThinkingDisplayMode(current_mode),
                                ctx,
                            );
                        });
                }
                AISettingsChangedEvent::PromptSubmissionMode { .. } => {
                    let current_mode = AISettings::as_ref(ctx).default_prompt_submission_mode;
                    me.default_prompt_submission_mode_dropdown
                        .update(ctx, |dropdown, ctx| {
                            dropdown.set_selected_by_action(
                                AISettingsPageAction::SetPromptSubmissionMode(current_mode),
                                ctx,
                            );
                        });
                }
                _ => (),
            }
            ctx.notify();
        });

        ctx.subscribe_to_model(&SessionSettings::handle(ctx), |_, _, event, ctx| {
            if let SessionSettingsChangedEvent::ShowModelSelectorsInPrompt { .. } = event {
                ctx.notify();
            }
        });

        ctx.subscribe_to_model(&InputSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        let current_permission =
            BlocklistAIPermissions::as_ref(ctx).active_permissions_profile(ctx, None);

        let apply_code_diffs_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);

            dropdown.set_items(
                vec![
                    DropdownItem::new(
                        "Agent decides",
                        AISettingsPageAction::SetApplyCodeDiffs(ActionPermission::AgentDecides),
                    ),
                    DropdownItem::new(
                        "Always allow",
                        AISettingsPageAction::SetApplyCodeDiffs(ActionPermission::AlwaysAllow),
                    ),
                    DropdownItem::new(
                        "Always ask",
                        AISettingsPageAction::SetApplyCodeDiffs(ActionPermission::AlwaysAsk),
                    ),
                ],
                ctx,
            );
            dropdown
        });
        Self::refresh_execution_profile_dropdown_menu(
            &apply_code_diffs_dropdown_menu,
            current_permission.apply_code_diffs,
            !AISettings::as_ref(ctx).is_code_diffs_permissions_editable(ctx),
            ctx,
        );

        let read_files_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_items(
                vec![
                    DropdownItem::new(
                        "Agent decides",
                        AISettingsPageAction::SetReadFiles(ActionPermission::AgentDecides),
                    ),
                    DropdownItem::new(
                        "Always allow",
                        AISettingsPageAction::SetReadFiles(ActionPermission::AlwaysAllow),
                    ),
                    DropdownItem::new(
                        "Always ask",
                        AISettingsPageAction::SetReadFiles(ActionPermission::AlwaysAsk),
                    ),
                ],
                ctx,
            );
            dropdown
        });
        Self::refresh_execution_profile_dropdown_menu(
            &read_files_dropdown_menu,
            current_permission.read_files,
            !AISettings::as_ref(ctx).is_read_files_permissions_editable(ctx),
            ctx,
        );

        let execute_commands_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_items(
                vec![
                    DropdownItem::new(
                        "Agent decides",
                        AISettingsPageAction::SetExecuteCommands(ActionPermission::AgentDecides),
                    ),
                    DropdownItem::new(
                        "Always allow",
                        AISettingsPageAction::SetExecuteCommands(ActionPermission::AlwaysAllow),
                    ),
                    DropdownItem::new(
                        "Always ask",
                        AISettingsPageAction::SetExecuteCommands(ActionPermission::AlwaysAsk),
                    ),
                ],
                ctx,
            );
            dropdown
        });
        Self::refresh_execution_profile_dropdown_menu(
            &execute_commands_dropdown_menu,
            current_permission.execute_commands,
            !AISettings::as_ref(ctx).is_execute_commands_permissions_editable(ctx),
            ctx,
        );

        let write_to_pty_autonomy_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_items(
                vec![
                    DropdownItem::new(
                        "Always allow",
                        AISettingsPageAction::SetWriteToPty(WriteToPtyPermission::AlwaysAllow),
                    ),
                    DropdownItem::new(
                        "Always ask",
                        AISettingsPageAction::SetWriteToPty(WriteToPtyPermission::AlwaysAsk),
                    ),
                    DropdownItem::new(
                        "Ask on first write",
                        AISettingsPageAction::SetWriteToPty(WriteToPtyPermission::AskOnFirstWrite),
                    ),
                ],
                ctx,
            );
            dropdown
        });
        Self::refresh_write_to_pty_dropdown_menu(
            &write_to_pty_autonomy_dropdown_menu,
            current_permission.write_to_pty,
            !AISettings::as_ref(ctx).is_write_to_pty_permissions_editable(ctx),
            ctx,
        );

        let mcp_permissions_dropdown_menu = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_items(
                vec![
                    DropdownItem::new(
                        "Agent decides",
                        AISettingsPageAction::SetMCPPermissions(ActionPermission::AgentDecides),
                    ),
                    DropdownItem::new(
                        "Always allow",
                        AISettingsPageAction::SetMCPPermissions(ActionPermission::AlwaysAllow),
                    ),
                    DropdownItem::new(
                        "Always ask",
                        AISettingsPageAction::SetMCPPermissions(ActionPermission::AlwaysAsk),
                    ),
                ],
                ctx,
            );
            dropdown
        });
        Self::refresh_execution_profile_dropdown_menu(
            &mcp_permissions_dropdown_menu,
            current_permission.mcp_permissions,
            !AISettings::as_ref(ctx).is_mcp_permission_editable(ctx),
            ctx,
        );

        let mcp_allowlist_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = FilterableDropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_header_to_static("Select MCP servers");
            dropdown
        });
        Self::refresh_mcp_allowlist_dropdown(&mcp_allowlist_dropdown, ctx);
        let mcp_allowlist_mouse_state_handles = BlocklistAIPermissions::as_ref(ctx)
            .get_mcp_allowlist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        let mcp_denylist_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = FilterableDropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_header_to_static("Select MCP servers");
            dropdown
        });
        Self::refresh_mcp_denylist_dropdown(&mcp_denylist_dropdown, ctx);
        let mcp_denylist_mouse_state_handles = BlocklistAIPermissions::as_ref(ctx)
            .get_mcp_denylist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        let command_execution_allowlist_mouse_state_handles = AISettings::as_ref(ctx)
            .agent_mode_command_execution_allowlist
            .value()
            .iter()
            .map(|_| Default::default())
            .collect();

        let command_execution_denylist_mouse_state_handles = AISettings::as_ref(ctx)
            .agent_mode_command_execution_denylist
            .value()
            .iter()
            .map(|_| Default::default())
            .collect();
        let cli_agent_footer_command_mouse_state_handles = AISettings::as_ref(ctx)
            .cli_agent_footer_enabled_commands
            .value()
            .keys()
            .map(|_| Default::default())
            .collect();

        let code_read_allowlist_mouse_state_handles = AISettings::as_ref(ctx)
            .agent_mode_coding_file_read_allowlist
            .value()
            .iter()
            .map(|_| Default::default())
            .collect();

        let directory_allowlist_mouse_state_handles = current_permission
            .directory_allowlist
            .iter()
            .map(|_| Default::default())
            .collect();

        let directory_allowlist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input = SubmittableTextInput::new(ctx).validate_on_submit(|s| {
                let expanded = host_native_absolute_path(s, &None, &None);
                Path::new(&expanded).is_dir()
            });
            input.set_placeholder_text("e.g. ~/code-repos/repo", ctx);
            input
        });

        Self::update_editor_interaction_state(
            directory_allowlist_editor.as_ref(ctx).editor().clone(),
            is_any_ai_enabled,
            ctx,
        );

        ctx.subscribe_to_view(&directory_allowlist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let expanded = host_native_absolute_path(s, &None, &None);
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();

                    model.add_to_directory_allowlist(*profile_id, &PathBuf::from(expanded), ctx);
                });
                ctx.notify();
            }
        });

        let org_denylist = BlocklistAIPermissions::get_org_execute_commands_denylist(ctx);
        let command_denylist_mouse_state_handles = current_permission
            .command_denylist
            .iter()
            .map(|_| Default::default())
            .collect();
        let command_denylist_tooltip_mouse_state_handles: Vec<MouseStateHandle> =
            org_denylist.iter().map(|_| Default::default()).collect();

        let command_denylist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input =
                SubmittableTextInput::new(ctx).validate_on_edit(|s| Regex::new(s).is_ok());
            input.set_placeholder_text("e.g. rm .*", ctx);
            input
        });
        Self::update_editor_interaction_state(
            command_denylist_editor.as_ref(ctx).editor().clone(),
            is_any_ai_enabled && !ai_autonomy_settings.has_override_for_execute_commands_denylist(),
            ctx,
        );

        ctx.subscribe_to_view(&command_denylist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let predicate = match AgentModeCommandExecutionPredicate::new_regex(s) {
                    Ok(regex) => regex,
                    Err(e) => {
                        log::warn!(
                            "Failed to convert string to regex for cmd execution denylist: {e}"
                        );
                        return;
                    }
                };
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.add_to_command_denylist(*profile_id, &predicate, ctx);
                });
                ctx.notify();
            }
        });

        let command_allowlist_mouse_state_handles = current_permission
            .command_allowlist
            .iter()
            .map(|_| Default::default())
            .collect();

        let command_allowlist_editor = ctx.add_typed_action_view(|ctx| {
            let mut input =
                SubmittableTextInput::new(ctx).validate_on_edit(|s| Regex::new(s).is_ok());
            input.set_placeholder_text("e.g. ls .*", ctx);
            input
        });
        Self::update_editor_interaction_state(
            command_allowlist_editor.as_ref(ctx).editor().clone(),
            is_any_ai_enabled
                && !ai_autonomy_settings.has_override_for_execute_commands_allowlist(),
            ctx,
        );

        ctx.subscribe_to_view(&command_allowlist_editor, |_, _, event, ctx| {
            if let SubmittableTextInputEvent::Submit(s) = event {
                let predicate = match AgentModeCommandExecutionPredicate::new_regex(s) {
                    Ok(regex) => regex,
                    Err(e) => {
                        log::warn!(
                            "Failed to convert string to regex for cmd execution allowlist: {e}"
                        );
                        return;
                    }
                };
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.add_to_command_allowlist(*profile_id, &predicate, ctx);
                });
                ctx.notify();
            }
        });

        let ai_request_model = AIRequestUsageModel::handle(ctx);
        ctx.subscribe_to_model(&ai_request_model, |me, _, event, ctx| {
            match event {
                AIRequestUsageModelEvent::RequestUsageUpdated => ctx.notify(),
                AIRequestUsageModelEvent::RequestBonusRefunded { .. } => ctx.notify(),
            }
            Self::refresh_base_model_menu(&me.base_model_dropdown, ctx);
            Self::refresh_coding_model_menu(&me.coding_model_dropdown, ctx);
        });

        let profile_views = Self::create_profile_views(ctx);

        let add_profile_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Add Profile", SecondaryTheme)
                .with_icon(Icon::Plus)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::CreateProfile);
                })
        });

        add_profile_button.update(ctx, |button, ctx| {
            button.set_disabled(!is_any_ai_enabled, ctx);
        });

        // Custom inference
        let custom_inference_controls_enabled =
            is_any_ai_enabled && UserWorkspaces::as_ref(ctx).is_custom_inference_enabled(ctx);
        let custom_inference_add_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("+ Add custom model", SecondaryTheme)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::OpenAddCustomEndpointModal);
                })
        });
        custom_inference_add_button.update(ctx, |button, ctx| {
            button.set_disabled(!custom_inference_controls_enabled, ctx);
        });

        let custom_endpoint_modal_body =
            ctx.add_typed_action_view(|ctx| CustomEndpointModal::new(None, None, ctx));
        ctx.subscribe_to_view(&custom_endpoint_modal_body, |me, _, event, ctx| {
            me.handle_custom_endpoint_modal_event(event, ctx);
        });

        let custom_endpoint_modal_view = ctx.add_typed_action_view(|ctx| {
            Modal::new(
                Some("Add custom endpoint".to_string()),
                custom_endpoint_modal_body.clone(),
                ctx,
            )
            .with_modal_style(UiComponentStyles {
                width: Some(560.),
                height: Some(600.),
                ..Default::default()
            })
            .with_header_style(UiComponentStyles {
                padding: Some(Coords {
                    top: 24.,
                    bottom: 0.,
                    left: 24.,
                    right: 24.,
                }),
                font_size: Some(16.),
                font_weight: Some(Weight::Bold),
                ..Default::default()
            })
            .with_body_style(UiComponentStyles {
                padding: Some(Coords {
                    top: 0.,
                    bottom: 24.,
                    left: 24.,
                    right: 24.,
                }),
                ..Default::default()
            })
            .with_background_opacity(100)
            .with_dismiss_on_click()
            .with_dismiss_keystroke(Keystroke::parse("escape").unwrap())
        });
        ctx.subscribe_to_view(&custom_endpoint_modal_view, |me, _, event, ctx| {
            me.handle_custom_endpoint_modal_close_event(event, ctx);
        });

        let custom_endpoint_modal_state =
            CustomEndpointModalViewState::new(ModalViewState::new(custom_endpoint_modal_view));

        let remove_custom_endpoint_confirmation_dialog =
            ctx.add_typed_action_view(RemoveCustomEndpointConfirmationDialog::new);
        ctx.subscribe_to_view(
            &remove_custom_endpoint_confirmation_dialog,
            |me, _, event, ctx| {
                me.handle_remove_custom_endpoint_confirmation_dialog_event(event, ctx);
            },
        );

        let custom_endpoint_edit_buttons = Self::create_custom_endpoint_edit_buttons(
            ApiKeyManager::as_ref(ctx).keys().custom_endpoints.len(),
            custom_inference_controls_enabled,
            ctx,
        );

        let agent_toolbar_inline_editor = ctx.add_typed_action_view(|ctx| {
            AgentToolbarInlineEditor::new(AgentToolbarEditorMode::AgentView, ctx)
        });
        let cli_agent_toolbar_inline_editor = ctx.add_typed_action_view(|ctx| {
            AgentToolbarInlineEditor::new(AgentToolbarEditorMode::CLIAgent, ctx)
        });

        #[cfg(feature = "local_fs")]
        let conversation_layout_dropdown = ctx.add_typed_action_view(|ctx| {
            use crate::util::file::external_editor::settings::OpenConversationPreference;

            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);

            let items = vec![
                DropdownItem::new(
                    "New Tab",
                    AISettingsPageAction::SetConversationLayout(OpenConversationPreference::NewTab),
                ),
                DropdownItem::new(
                    "Split Pane",
                    AISettingsPageAction::SetConversationLayout(
                        OpenConversationPreference::SplitPane,
                    ),
                ),
            ];
            dropdown.set_items(items, ctx);

            let current = *crate::util::file::external_editor::EditorSettings::as_ref(ctx)
                .open_conversation_layout_preference;
            match current {
                OpenConversationPreference::NewTab => dropdown.set_selected_by_name("New Tab", ctx),
                OpenConversationPreference::SplitPane => {
                    dropdown.set_selected_by_name("Split Pane", ctx)
                }
            };
            dropdown
        });

        Self {
            page: Self::build_page(None, ctx),
            active_subpage: None,
            voice_input_toggle_key_dropdown,
            autodetection_denylist_editor,
            local_only_icon_tooltip_states: Default::default(),
            command_execution_allowlist_editor,
            command_execution_denylist_editor,
            command_execution_allowlist_mouse_state_handles,
            command_execution_denylist_mouse_state_handles,
            cli_agent_footer_command_editor,
            cli_agent_footer_command_mouse_state_handles,
            cli_agent_footer_command_agent_dropdowns: Self::create_cli_agent_dropdowns(ctx),
            cli_agent_builtin_prompt_editors,
            cli_agent_builtin_prompt_mode_dropdowns,
            cli_agent_api_profile_preset_dropdown,
            cli_agent_api_profile_selected_preset_id,
            cli_agent_api_takeover_toggle: SwitchStateHandle::default(),
            cli_agent_api_profile_agent_dropdown,
            cli_agent_api_profile_environment_dropdown,
            cli_agent_api_profile_draft_agent,
            cli_agent_api_profile_draft_environment_id,
            cli_agent_api_profile_editing_profile_id: initial_cli_agent_api_profile
                .as_ref()
                .map(|profile| profile.id.clone()),
            cli_agent_api_profile_editor_open: false,
            cli_agent_api_profile_modal_scroll_state: ClippedScrollStateHandle::default(),
            cli_agent_api_profile_name_editor,
            cli_agent_api_profile_base_url_editor,
            cli_agent_api_profile_openai_base_url_editor,
            cli_agent_api_profile_api_format_editor,
            cli_agent_api_profile_auth_env_var_editor,
            cli_agent_api_profile_full_url_mode_editor,
            cli_agent_api_profile_api_key_editor,
            cli_agent_api_profile_model_editor,
            cli_agent_api_profile_model_catalog_editor,
            cli_agent_api_profile_model_mappings_editor,
            cli_agent_api_profile_priority_editor,
            cli_agent_api_profile_input_cost_editor,
            cli_agent_api_profile_output_cost_editor,
            cli_agent_api_profile_extra_env_editor,
            cli_agent_api_profile_fetched_models: initial_cli_agent_api_fetched_models,
            cli_agent_api_profile_selected_fetched_model:
                initial_cli_agent_api_selected_fetched_model,
            cli_agent_api_profile_fetch_models_error: None,
            cli_agent_api_profile_fetching_models: false,
            cli_agent_api_profile_fetch_models_request_id: 0,
            cli_agent_api_profile_model_picker_dropdown,
            cli_agent_api_profile_add_model_open: false,
            cli_agent_api_profile_model_mapping_open: false,
            cli_agent_api_profile_save_feedback: None,
            cli_agent_api_profile_save_feedback_is_error: false,
            cli_agent_api_profile_save_feedback_generation: 0,
            cli_agent_api_profile_open_add_button,
            cli_agent_api_profile_add_button,
            cli_agent_api_profile_cancel_edit_button,
            cli_agent_api_profile_fetch_models_button,
            cli_agent_api_profile_toggle_add_model_button,
            cli_agent_api_profile_add_manual_model_button,
            cli_agent_api_profile_add_selected_model_button,
            cli_agent_api_profiles_json_editor,
            cli_agent_api_profile_mouse_state_handles,
            cli_agent_api_profile_edit_mouse_state_handles,
            cli_agent_api_profile_check_mouse_state_handles,
            cli_agent_api_profile_toggle_mouse_state_handles,
            cli_agent_api_profile_remove_mouse_state_handles,
            agent_session_id_search_editor,
            agent_session_id_editing_record_id: None,
            agent_session_id_agent_editor,
            agent_session_id_project_editor,
            agent_session_id_value_editor,
            agent_session_id_title_editor,
            agent_toolbar_inline_editor,
            cli_agent_toolbar_inline_editor,
            base_model_dropdown,
            coding_model_dropdown,
            context_window_slider_state,
            context_window_editor,
            last_synced_context_window_editor_value,
            dragged_context_window_value: None,
            autonomy_dropdown_menu,
            code_read_allowlist_editor,
            code_read_autonomy_dropdown_menu,
            code_read_allowlist_mouse_state_handles,
            apply_code_diffs_dropdown_menu,
            read_files_dropdown_menu,
            execute_commands_dropdown_menu,
            write_to_pty_autonomy_dropdown_menu,
            mcp_permissions_dropdown_menu,
            directory_allowlist_mouse_state_handles,
            directory_allowlist_editor,
            command_denylist_mouse_state_handles,
            command_denylist_tooltip_mouse_state_handles,
            command_denylist_editor,
            command_allowlist_mouse_state_handles,
            command_allowlist_editor,
            mcp_allowlist_dropdown,
            mcp_allowlist_mouse_state_handles,
            mcp_denylist_dropdown,
            mcp_denylist_mouse_state_handles,
            thinking_display_mode_dropdown,
            default_prompt_submission_mode_dropdown,
            #[cfg(feature = "local_fs")]
            conversation_layout_dropdown,
            profile_views,
            add_profile_button,
            custom_endpoint_modal_state,
            remove_custom_endpoint_confirmation_dialog,
            pending_remove_custom_endpoint_index: None,
            custom_inference_add_button,
            custom_endpoint_edit_buttons,
        }
    }

    fn update_voice_input_dropdown_enablement(&mut self, ctx: &mut ViewContext<Self>) {
        let is_voice_enabled = AISettings::as_ref(ctx).is_voice_input_enabled(ctx);
        self.voice_input_toggle_key_dropdown
            .update(ctx, |dropdown, ctx| {
                if is_voice_enabled {
                    dropdown.set_enabled(ctx);
                } else {
                    dropdown.set_disabled(ctx);
                }
            });
        ctx.notify();
    }

    pub fn get_modal_content(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if self.cli_agent_api_profile_editor_open {
            Some(self.render_cli_agent_api_profile_editor_modal(app))
        } else if self.custom_endpoint_modal_state.is_open() {
            Some(self.custom_endpoint_modal_state.render())
        } else if self
            .remove_custom_endpoint_confirmation_dialog
            .as_ref(app)
            .is_visible()
        {
            Some(ChildView::new(&self.remove_custom_endpoint_confirmation_dialog).finish())
        } else {
            None
        }
    }

    fn render_cli_agent_api_profile_editor_modal(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let is_editing = self.cli_agent_api_profile_editing_profile_id.is_some();
        let title = if is_editing {
            "Edit Agent API endpoint"
        } else {
            "Add Agent API endpoint"
        };

        let close_button = appearance
            .ui_builder()
            .close_button(22., MouseStateHandle::default())
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::CancelEditCLIAgentApiProfile);
            })
            .finish();
        let header = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_child(
                    Text::new_inline(title, appearance.header_font_family(), 16.)
                        .with_color(theme.active_ui_text_color().into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                )
                .with_child(close_button)
                .finish(),
        )
        .with_horizontal_padding(18.)
        .with_vertical_padding(12.)
        .with_border(Border::bottom(1.).with_border_fill(theme.outline()))
        .finish();

        let form = CLIAgentApiProfilesWidget.render_profile_form(self, appearance, app);
        let scrollable = ClippedScrollable::vertical(
            self.cli_agent_api_profile_modal_scroll_state.clone(),
            form,
            ScrollbarWidth::Custom(4.),
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            ElementFill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let body = ConstrainedBox::new(
            Container::new(scrollable)
                .with_uniform_padding(14.)
                .with_background(theme.surface_2())
                .finish(),
        )
        .with_max_height(460.)
        .finish();

        let footer_actions = Flex::row()
            .with_spacing(8.)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                self.cli_agent_api_profile_cancel_edit_button
                    .as_ref(app)
                    .render(app),
            )
            .with_child(
                self.cli_agent_api_profile_add_button
                    .as_ref(app)
                    .render(app),
            )
            .finish();
        let footer = Container::new(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_size(MainAxisSize::Max)
                .with_child(
                    self.cli_agent_api_profile_fetch_models_button
                        .as_ref(app)
                        .render(app),
                )
                .with_child(footer_actions)
                .finish(),
        )
        .with_horizontal_padding(18.)
        .with_vertical_padding(10.)
        .with_border(Border::top(1.).with_border_fill(theme.outline()))
        .finish();

        let modal = ConstrainedBox::new(
            Container::new(
                Flex::column()
                    .with_child(header)
                    .with_child(body)
                    .with_child(footer)
                    .finish(),
            )
            .with_background(theme.surface_1())
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .finish(),
        )
        .with_max_width(780.)
        .with_max_height(620.)
        .finish();

        Container::new(Align::new(modal).finish())
            .with_background_color(ColorU::new(0, 0, 0, 179))
            .with_corner_radius(app.windows().window_corner_radius())
            .finish()
    }

    fn sync_custom_endpoint_buttons(&mut self, ctx: &mut ViewContext<Self>) {
        let enabled = Self::can_use_custom_inference_controls(ctx);

        self.custom_inference_add_button.update(ctx, |button, ctx| {
            button.set_disabled(!enabled, ctx);
        });

        let endpoint_count = ApiKeyManager::as_ref(ctx).keys().custom_endpoints.len();
        if self.custom_endpoint_edit_buttons.len() != endpoint_count {
            self.custom_endpoint_edit_buttons =
                Self::create_custom_endpoint_edit_buttons(endpoint_count, enabled, ctx);
        } else {
            for button in &self.custom_endpoint_edit_buttons {
                button.update(ctx, |button, ctx| {
                    button.set_disabled(!enabled, ctx);
                });
            }
        }
    }

    fn create_custom_endpoint_edit_buttons(
        count: usize,
        enabled: bool,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<ActionButton>> {
        (0..count)
            .map(|index| {
                let button = ctx.add_typed_action_view(move |_| {
                    ActionButton::new("Edit", SecondaryTheme)
                        .with_icon(Icon::Pencil)
                        .with_size(ButtonSize::Small)
                        .on_click(move |ctx| {
                            ctx.dispatch_typed_action(
                                AISettingsPageAction::OpenEditCustomEndpointModal(index),
                            );
                        })
                });
                button.update(ctx, |button, ctx| {
                    button.set_disabled(!enabled, ctx);
                });
                button
            })
            .collect()
    }
    fn can_use_custom_inference_controls(app: &AppContext) -> bool {
        FeatureFlag::CustomInferenceEndpoints.is_enabled()
            && AISettings::as_ref(app).is_any_ai_enabled(app)
            && UserWorkspaces::as_ref(app).is_custom_inference_enabled(app)
    }

    fn show_add_custom_endpoint_modal(&mut self, ctx: &mut ViewContext<Self>) {
        if !Self::can_use_custom_inference_controls(ctx) {
            return;
        }
        self.remove_custom_endpoint_confirmation_dialog
            .update(ctx, |dialog, ctx| {
                dialog.hide(ctx);
            });
        self.pending_remove_custom_endpoint_index = None;

        self.custom_endpoint_modal_state
            .set_title(Some("Add custom endpoint".to_string()), ctx);
        self.custom_endpoint_modal_state.prefill(None, None, ctx);
        self.custom_endpoint_modal_state.open(ctx);
        ctx.emit(AISettingsPageEvent::ShowModal);
        ctx.notify();
    }

    fn show_edit_custom_endpoint_modal(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if !Self::can_use_custom_inference_controls(ctx) {
            return;
        }
        let endpoint = ApiKeyManager::as_ref(ctx)
            .keys()
            .custom_endpoints
            .get(index)
            .cloned();
        if endpoint.is_none() {
            return;
        }

        self.remove_custom_endpoint_confirmation_dialog
            .update(ctx, |dialog, ctx| {
                dialog.hide(ctx);
            });
        self.pending_remove_custom_endpoint_index = None;

        self.custom_endpoint_modal_state
            .set_title(Some("Edit custom endpoint".to_string()), ctx);
        self.custom_endpoint_modal_state
            .prefill(endpoint.as_ref(), Some(index), ctx);
        self.custom_endpoint_modal_state.open(ctx);
        ctx.emit(AISettingsPageEvent::ShowModal);
        ctx.notify();
    }

    fn hide_custom_endpoint_modal(&mut self, ctx: &mut ViewContext<Self>) {
        self.custom_endpoint_modal_state.close(ctx);
        ctx.emit(AISettingsPageEvent::HideModal);
        ctx.notify();
    }

    fn handle_custom_endpoint_modal_close_event(
        &mut self,
        event: &ModalEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            ModalEvent::Close => {
                self.hide_custom_endpoint_modal(ctx);
            }
        }
    }

    fn handle_custom_endpoint_modal_event(
        &mut self,
        event: &CustomEndpointModalEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            CustomEndpointModalEvent::Close => {
                self.hide_custom_endpoint_modal(ctx);
            }
            CustomEndpointModalEvent::AddEndpoint {
                name,
                url,
                api_key,
                models,
            } => {
                if !Self::can_use_custom_inference_controls(ctx) {
                    self.hide_custom_endpoint_modal(ctx);
                    return;
                }
                ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.add_custom_endpoint(
                        name.clone(),
                        url.clone(),
                        api_key.clone(),
                        models.clone(),
                        ctx,
                    );
                });
                self.hide_custom_endpoint_modal(ctx);

                let window_id = ctx.window_id();
                crate::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = crate::view_components::DismissibleToast::success(
                        "Endpoint added".to_string(),
                    );
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx);
                });
                ctx.notify();
            }
            CustomEndpointModalEvent::SaveEndpoint {
                index,
                name,
                url,
                api_key,
                models,
            } => {
                if !Self::can_use_custom_inference_controls(ctx) {
                    self.hide_custom_endpoint_modal(ctx);
                    return;
                }
                ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.save_custom_endpoint(
                        *index,
                        name.clone(),
                        url.clone(),
                        api_key.clone(),
                        models.clone(),
                        ctx,
                    );
                });
                self.hide_custom_endpoint_modal(ctx);

                let window_id = ctx.window_id();
                crate::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = crate::view_components::DismissibleToast::success(
                        "Endpoint saved".to_string(),
                    );
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx);
                });
                ctx.notify();
            }
            CustomEndpointModalEvent::RemoveEndpoint { index } => {
                if !Self::can_use_custom_inference_controls(ctx) {
                    self.hide_custom_endpoint_modal(ctx);
                    return;
                }
                self.hide_custom_endpoint_modal(ctx);
                self.show_remove_custom_endpoint_confirmation_dialog(*index, ctx);
            }
        }
    }

    fn show_remove_custom_endpoint_confirmation_dialog(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if !Self::can_use_custom_inference_controls(ctx) {
            return;
        }
        let endpoint = ApiKeyManager::as_ref(ctx)
            .keys()
            .custom_endpoints
            .get(index)
            .cloned();
        let Some(endpoint) = endpoint else {
            return;
        };

        let model_labels = endpoint
            .models
            .iter()
            .map(|model| model.alias.clone().unwrap_or_else(|| model.name.clone()))
            .filter(|s| !s.trim().is_empty())
            .collect();

        self.pending_remove_custom_endpoint_index = Some(index);
        self.remove_custom_endpoint_confirmation_dialog
            .update(ctx, |dialog, ctx| {
                dialog.show(index, endpoint.name.clone(), model_labels, ctx);
            });
        ctx.notify();
    }

    fn handle_remove_custom_endpoint_confirmation_dialog_event(
        &mut self,
        event: &RemoveCustomEndpointConfirmationDialogEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            RemoveCustomEndpointConfirmationDialogEvent::Cancel => {
                self.pending_remove_custom_endpoint_index = None;
                self.remove_custom_endpoint_confirmation_dialog
                    .update(ctx, |dialog, ctx| {
                        dialog.hide(ctx);
                    });
                ctx.notify();
            }
            RemoveCustomEndpointConfirmationDialogEvent::Confirm(index) => {
                if !Self::can_use_custom_inference_controls(ctx) {
                    self.pending_remove_custom_endpoint_index = None;
                    self.remove_custom_endpoint_confirmation_dialog
                        .update(ctx, |dialog, ctx| {
                            dialog.hide(ctx);
                        });
                    ctx.notify();
                    return;
                }
                ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.remove_custom_endpoint(*index, ctx);
                });
                self.pending_remove_custom_endpoint_index = None;
                self.remove_custom_endpoint_confirmation_dialog
                    .update(ctx, |dialog, ctx| {
                        dialog.hide(ctx);
                    });
                self.sync_custom_endpoint_buttons(ctx);

                let window_id = ctx.window_id();
                crate::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = crate::view_components::DismissibleToast::success(
                        "Endpoint removed".to_string(),
                    );
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx);
                });
                ctx.notify();
            }
        }
    }

    /// Set the active subpage and rebuild the widget list to show only relevant widgets.
    pub fn set_active_subpage(&mut self, subpage: Option<AISubpage>, ctx: &mut ViewContext<Self>) {
        if self.active_subpage != subpage {
            self.active_subpage = subpage;
            self.page = Self::build_page(subpage, ctx);
            ctx.notify();
        }
    }

    fn build_page(subpage: Option<AISubpage>, ctx: &mut ViewContext<Self>) -> PageType<Self> {
        let ai_settings = AISettings::as_ref(ctx);

        let mut widgets: Vec<Box<dyn SettingsWidget<View = AISettingsPageView>>> = Vec::new();

        // When viewing a specific subpage, only include its widgets.
        // When subpage is None (legacy/backward-compat), show all widgets.
        match subpage {
            None => {
                // Full page: all widgets (legacy behavior)
                widgets.push(Box::new(GlobalAIWidget::default()));
                if !FeatureFlag::UsageBasedPricing.is_enabled() {
                    widgets.push(Box::new(UsageWidget::default()));
                }
                if ai_settings
                    .intelligent_autosuggestions_enabled_internal
                    .is_supported_on_current_platform()
                    || ai_settings
                        .prompt_suggestions_enabled_internal
                        .is_supported_on_current_platform()
                    || (FeatureFlag::PredictAMQueries.is_enabled()
                        && ai_settings
                            .natural_language_autosuggestions_enabled_internal
                            .is_supported_on_current_platform())
                    || (FeatureFlag::SharedBlockTitleGeneration.is_enabled()
                        && ai_settings
                            .shared_block_title_generation_enabled_internal
                            .is_supported_on_current_platform())
                    || (FeatureFlag::GitOperationsInCodeReview.is_enabled()
                        && ai_settings
                            .git_operations_autogen_enabled_internal
                            .is_supported_on_current_platform())
                {
                    widgets.push(Box::new(ActiveAIWidget::default()));
                }
                widgets.push(Box::new(AgentsWidget::default()));
                widgets.push(Box::new(AIInputWidget::default()));
                if MCPServersWidget::should_show_mcp() {
                    widgets.push(Box::new(MCPServersWidget::default()));
                }
                if FeatureFlag::AIRules.is_enabled() {
                    widgets.push(Box::new(AIFactWidget::default()));
                }
                if cfg!(feature = "voice_input")
                    && ai_settings
                        .voice_input_enabled_internal
                        .is_supported_on_current_platform()
                {
                    widgets.push(Box::new(VoiceWidget::default()));
                }
                widgets.push(Box::new(CloudHandoffWidget::default()));
                widgets.push(Box::new(CLIAgentWidget::default()));
                widgets.push(Box::new(CLIAgentBuiltinPromptsWidget::default()));
                widgets.push(Box::new(ApiKeysWidget::new(ctx)));
                widgets.push(Box::new(AwsBedrockWidget::new(ctx)));
                widgets.push(Box::new(AgentAttributionWidget::default()));
                widgets.push(Box::new(OtherAIWidget::default()));
                if FeatureFlag::AgentModeComputerUse.is_enabled() {
                    widgets.push(Box::new(CloudAgentComputerUseWidget::default()));
                }
            }
            Some(AISubpage::WarpAgent) => {
                // Oz page: global toggle + Active AI + Input + Other
                widgets.push(Box::new(GlobalAIWidget::default()));
                if ai_settings
                    .intelligent_autosuggestions_enabled_internal
                    .is_supported_on_current_platform()
                    || ai_settings
                        .prompt_suggestions_enabled_internal
                        .is_supported_on_current_platform()
                    || (FeatureFlag::PredictAMQueries.is_enabled()
                        && ai_settings
                            .natural_language_autosuggestions_enabled_internal
                            .is_supported_on_current_platform())
                    || (FeatureFlag::SharedBlockTitleGeneration.is_enabled()
                        && ai_settings
                            .shared_block_title_generation_enabled_internal
                            .is_supported_on_current_platform())
                    || (FeatureFlag::GitOperationsInCodeReview.is_enabled()
                        && ai_settings
                            .git_operations_autogen_enabled_internal
                            .is_supported_on_current_platform())
                {
                    widgets.push(Box::new(ActiveAIWidget::default()));
                }
                widgets.push(Box::new(AIInputWidget::default()));
                let voice_supported = cfg!(feature = "voice_input")
                    && ai_settings
                        .voice_input_enabled_internal
                        .is_supported_on_current_platform();
                if voice_supported {
                    widgets.push(Box::new(VoiceWidget::default()));
                }
                widgets.push(Box::new(CloudHandoffWidget::default()));
                widgets.push(Box::new(ApiKeysWidget::new(ctx)));
                widgets.push(Box::new(AwsBedrockWidget::new(ctx)));
                widgets.push(Box::new(AgentAttributionWidget::default()));
                widgets.push(Box::new(OtherAIWidget::default()));
                if FeatureFlag::AgentModeComputerUse.is_enabled() {
                    widgets.push(Box::new(CloudAgentComputerUseWidget::default()));
                }
            }
            Some(AISubpage::Profiles) => {
                if !FeatureFlag::UsageBasedPricing.is_enabled() {
                    widgets.push(Box::new(UsageWidget::default()));
                }
                widgets.push(Box::new(AgentsWidget::default()));
            }
            Some(AISubpage::Knowledge) => {
                if FeatureFlag::AIRules.is_enabled() {
                    widgets.push(Box::new(AIFactWidget::default()));
                }
            }
            Some(AISubpage::ThirdPartyCLIAgents) => {
                widgets.push(Box::new(CLIAgentWidget::default()));
            }
            Some(AISubpage::AgentApiProfiles) => {
                widgets.push(Box::new(CLIAgentApiProfilesWidget::default()));
            }
            Some(AISubpage::SessionIds) => {
                widgets.push(Box::new(CLIAgentSessionIdsWidget::default()));
            }
            Some(AISubpage::BuiltinPrompts) => {
                widgets.push(Box::new(CLIAgentBuiltinPromptsWidget::default()));
            }
        }

        // Subpage widgets render their own subheader-sized titles internally,
        // so we don't pass a page-level title to PageType.
        let title: Option<&str> = None;
        PageType::new_uncategorized(widgets, title)
    }

    fn handle_context_window_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => {
                if !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                    self.sync_context_window_editor(ctx, true);
                    return;
                }
                if let Some(cw) = Self::configurable_context_window(ctx) {
                    let buffer_text = self.context_window_editor.as_ref(ctx).buffer_text(ctx);
                    let cleaned: String = buffer_text
                        .chars()
                        .filter(|c| !c.is_whitespace() && *c != ',')
                        .collect();
                    if let Ok(parsed) = cleaned.parse::<u32>() {
                        let clamped = parsed.clamp(cw.min, cw.max);
                        if Some(clamped) != Self::current_context_window_display_value(ctx) {
                            AIExecutionProfilesModel::handle(ctx).update(
                                ctx,
                                |profiles_model, ctx| {
                                    let profile_id = *profiles_model.active_profile(None, ctx).id();
                                    profiles_model.set_context_window_limit(
                                        profile_id,
                                        Some(clamped),
                                        ctx,
                                    );
                                },
                            );
                        }
                    }
                }
                self.sync_context_window_editor(ctx, true);
                if let EditorEvent::Enter = event {
                    ctx.emit(AISettingsPageEvent::FocusModal);
                }
                ctx.notify();
            }
            EditorEvent::Escape => ctx.emit(AISettingsPageEvent::FocusModal),
            _ => {}
        }
    }

    fn active_profile_data(app: &AppContext) -> AIExecutionProfile {
        AIExecutionProfilesModel::as_ref(app)
            .active_profile(None, app)
            .data()
            .clone()
    }

    fn configurable_context_window(app: &AppContext) -> Option<LLMContextWindow> {
        Self::active_profile_data(app).configurable_context_window(app)
    }

    fn current_context_window_display_value(app: &AppContext) -> Option<u32> {
        Self::active_profile_data(app).context_window_display_value(app)
    }

    fn initial_context_window_value(app: &AppContext) -> u32 {
        Self::current_context_window_display_value(app).unwrap_or_else(|| {
            LLMPreferences::as_ref(app)
                .get_active_base_model(app, None)
                .context_window
                .default_max
        })
    }

    fn sync_context_window_editor(&mut self, ctx: &mut ViewContext<Self>, force: bool) {
        self.dragged_context_window_value = None;
        let Some(value) = Self::current_context_window_display_value(ctx) else {
            self.last_synced_context_window_editor_value = None;
            self.context_window_slider_state.reset_offset();
            ctx.notify();
            return;
        };

        let formatted = value.to_string();
        let should_update = if force {
            true
        } else {
            match self.last_synced_context_window_editor_value {
                Some(last_value) => {
                    self.context_window_editor.as_ref(ctx).buffer_text(ctx)
                        == last_value.to_string()
                }
                None => true,
            }
        };

        if should_update {
            self.context_window_editor.update(ctx, |editor, ctx| {
                if editor.buffer_text(ctx) != formatted {
                    editor.system_reset_buffer_text(&formatted, ctx);
                }
            });
            self.last_synced_context_window_editor_value = Some(value);
            self.context_window_slider_state.reset_offset();
            ctx.notify();
        }
    }

    fn handle_detection_denylist_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => {
                let buffer_text = self
                    .autodetection_denylist_editor
                    .as_ref(ctx)
                    .buffer_text(ctx);
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    if let Err(e) = settings
                        .autodetection_command_denylist
                        .set_value(buffer_text, ctx)
                    {
                        log::warn!("Failed to set AI autodetection blacklist commands: {e:?}");
                    }
                })
            }
            EditorEvent::Escape => ctx.emit(AISettingsPageEvent::FocusModal),
            _ => {}
        }
    }

    fn update_editor_interaction_state(
        editor: ViewHandle<EditorView>,
        is_enabled: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        editor.update(ctx, |editor, ctx| {
            let interaction_state = if is_enabled {
                InteractionState::Editable
            } else {
                InteractionState::Disabled
            };
            editor.set_interaction_state(interaction_state, ctx);
            ctx.notify();
        })
    }

    pub fn refresh_base_model_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            let disabled_by_ai_toggle = !AISettings::as_ref(ctx).is_any_ai_enabled(ctx);

            if disabled_by_ai_toggle {
                menu.set_disabled(ctx);
            } else {
                menu.set_enabled(ctx);
            }

            let choices = LLMPreferences::as_ref(ctx)
                .get_base_llm_choices_for_agent_mode(ctx)
                .collect_vec();

            let items = available_model_menu_items(
                choices,
                |llm| {
                    DropdownAction::select_action_and_close(AISettingsPageAction::SetBaseModel(
                        llm.id.clone(),
                    ))
                },
                None,
                None,
                false,
                false,
                ctx,
            );
            menu.set_rich_items(items, ctx);

            let active = LLMPreferences::as_ref(ctx).get_active_base_model(ctx, None);
            menu.set_selected_by_action(AISettingsPageAction::SetBaseModel(active.id.clone()), ctx);
            ctx.notify();
        });
        ctx.notify();
    }

    pub fn refresh_coding_model_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            let disabled_by_ai_toggle = !AISettings::as_ref(ctx).is_any_ai_enabled(ctx);

            if disabled_by_ai_toggle {
                menu.set_disabled(ctx);
            } else {
                menu.set_enabled(ctx);
            }

            let choices = LLMPreferences::as_ref(ctx)
                .get_coding_llm_choices(ctx)
                .collect_vec();

            let items = available_model_menu_items(
                choices,
                |llm| {
                    DropdownAction::select_action_and_close(AISettingsPageAction::SetCodingModel(
                        llm.id.clone(),
                    ))
                },
                None,
                None,
                false,
                false,
                ctx,
            );
            menu.set_rich_items(items, ctx);
            let active = LLMPreferences::as_ref(ctx).get_active_coding_model(ctx, None);

            menu.set_selected_by_action(
                AISettingsPageAction::SetCodingModel(active.id.clone()),
                ctx,
            );
            ctx.notify();
        });
        ctx.notify();
    }

    fn refresh_autonomy_dropdown_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            if AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                menu.set_enabled(ctx);
            } else {
                menu.set_disabled(ctx);
            }

            menu.set_items(
                vec![
                    DropdownItem::new(
                        "Read only",
                        AISettingsPageAction::SetAutonomyReadonlyCommandsSetting,
                    ),
                    DropdownItem::new(
                        "Supervised",
                        AISettingsPageAction::SetAutonomySupervisedSetting,
                    ),
                ],
                ctx,
            );
            let active = if *AISettings::as_ref(ctx).agent_mode_execute_read_only_commands {
                0
            } else {
                1
            };
            menu.set_selected_by_index(active, ctx);
            ctx.notify();
        });
        ctx.notify();
    }

    fn refresh_all_execution_profile_ui(&self, ctx: &mut ViewContext<Self>) {
        let permissions = BlocklistAIPermissions::handle(ctx);

        let apply_code_diffs_setting = permissions
            .as_ref(ctx)
            .get_apply_code_diffs_setting(ctx, None);
        Self::refresh_execution_profile_dropdown_menu(
            &self.apply_code_diffs_dropdown_menu,
            apply_code_diffs_setting,
            !AISettings::as_ref(ctx).is_code_diffs_permissions_editable(ctx),
            ctx,
        );

        let read_files_setting = permissions.as_ref(ctx).get_read_files_setting(ctx, None);
        Self::refresh_execution_profile_dropdown_menu(
            &self.read_files_dropdown_menu,
            read_files_setting,
            !AISettings::as_ref(ctx).is_read_files_permissions_editable(ctx),
            ctx,
        );

        let execute_commands_setting: ActionPermission = permissions
            .as_ref(ctx)
            .get_execute_commands_setting(ctx, None);
        Self::refresh_execution_profile_dropdown_menu(
            &self.execute_commands_dropdown_menu,
            execute_commands_setting,
            !AISettings::as_ref(ctx).is_execute_commands_permissions_editable(ctx),
            ctx,
        );

        let write_to_pty_setting: WriteToPtyPermission =
            permissions.as_ref(ctx).get_write_to_pty_setting(ctx, None);
        Self::refresh_write_to_pty_dropdown_menu(
            &self.write_to_pty_autonomy_dropdown_menu,
            write_to_pty_setting,
            !AISettings::as_ref(ctx).is_write_to_pty_permissions_editable(ctx),
            ctx,
        );

        let mcp_permissions_setting = permissions
            .as_ref(ctx)
            .get_mcp_permissions_setting(ctx, None);
        Self::refresh_execution_profile_dropdown_menu(
            &self.mcp_permissions_dropdown_menu,
            mcp_permissions_setting,
            !AISettings::as_ref(ctx).is_mcp_permission_editable(ctx),
            ctx,
        );
        Self::refresh_mcp_allowlist_dropdown(&self.mcp_allowlist_dropdown, ctx);
        Self::refresh_mcp_denylist_dropdown(&self.mcp_denylist_dropdown, ctx);

        let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
        self.add_profile_button.update(ctx, |button, ctx| {
            button.set_disabled(!is_any_ai_enabled, ctx);
        });
    }

    fn reset_execution_profile_mouse_state_handles(&mut self, ctx: &mut ViewContext<Self>) {
        let blocklist_permissions = BlocklistAIPermissions::as_ref(ctx);

        self.directory_allowlist_mouse_state_handles = blocklist_permissions
            .get_read_files_allowlist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        self.command_denylist_mouse_state_handles = blocklist_permissions
            .get_execute_commands_denylist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        let org_denylist = BlocklistAIPermissions::get_org_execute_commands_denylist(ctx);
        self.command_denylist_tooltip_mouse_state_handles =
            org_denylist.iter().map(|_| Default::default()).collect();

        self.command_allowlist_mouse_state_handles = blocklist_permissions
            .get_execute_commands_allowlist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        self.mcp_allowlist_mouse_state_handles = blocklist_permissions
            .get_mcp_allowlist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();

        self.mcp_denylist_mouse_state_handles = blocklist_permissions
            .get_mcp_denylist(ctx, None)
            .iter()
            .map(|_| Default::default())
            .collect();
    }

    fn refresh_execution_profile_dropdown_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        current_permission: ActionPermission,
        disabled: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            if !disabled {
                menu.set_enabled(ctx);
            } else {
                menu.set_disabled(ctx);
            }

            let active = match current_permission {
                ActionPermission::AgentDecides | ActionPermission::Unknown => 0,
                ActionPermission::AlwaysAllow => 1,
                ActionPermission::AlwaysAsk => 2,
            };

            menu.set_selected_by_index(active, ctx);
            ctx.notify();
        });
        ctx.notify();
    }

    fn refresh_write_to_pty_dropdown_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        current_permission: WriteToPtyPermission,
        disabled: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            if !disabled {
                menu.set_enabled(ctx);
            } else {
                menu.set_disabled(ctx);
            }

            let active = match current_permission {
                WriteToPtyPermission::AlwaysAllow => 0,
                WriteToPtyPermission::AlwaysAsk | WriteToPtyPermission::Unknown => 1,
                WriteToPtyPermission::AskOnFirstWrite => 2,
            };

            menu.set_selected_by_index(active, ctx);
            ctx.notify();
        });
        ctx.notify();
    }

    /// Currently, the coding permissions only support "read" access.
    fn refresh_code_read_autonomy_dropdown_menu(
        menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        menu.update(ctx, |menu, ctx| {
            if AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                menu.set_enabled(ctx);
            } else {
                menu.set_disabled(ctx);
            }

            menu.set_items(
                AgentModeCodingPermissionsType::iter()
                    .map(|t| {
                        let display = match t {
                            AgentModeCodingPermissionsType::AlwaysAskBeforeReading => "Always ask",
                            AgentModeCodingPermissionsType::AlwaysAllowReading => "Always allow",
                            AgentModeCodingPermissionsType::AllowReadingSpecificFiles => {
                                "Allow in specific directories"
                            }
                        };
                        DropdownItem::new(display, AISettingsPageAction::SetCodingPermission(t))
                    })
                    .collect(),
                ctx,
            );
            let ai_settings = AISettings::as_ref(ctx);

            let active = if *ai_settings.agent_mode_execute_read_only_commands {
                menu.set_disabled(ctx);
                AgentModeCodingPermissionsType::AlwaysAllowReading
            } else {
                *ai_settings.agent_mode_coding_permissions
            };
            menu.set_selected_by_action(AISettingsPageAction::SetCodingPermission(active), ctx);
            ctx.notify();
        });
        ctx.notify();
    }

    fn get_non_allowlisted_or_denylisted_mcp_servers(
        ctx: &mut ViewContext<Self>,
    ) -> Vec<(uuid::Uuid, String)> {
        let all_mcp_servers = TemplatableMCPServerManager::get_all_cloud_synced_mcp_servers(ctx);
        let already_allowlisted_mcp_servers =
            BlocklistAIPermissions::as_ref(ctx).get_mcp_allowlist(ctx, None);
        let already_denylisted_mcp_servers =
            BlocklistAIPermissions::as_ref(ctx).get_mcp_denylist(ctx, None);

        all_mcp_servers
            .into_iter()
            .filter(|(uuid, _)| {
                let is_allowlisted = already_allowlisted_mcp_servers.contains(uuid);
                let is_denylisted = already_denylisted_mcp_servers.contains(uuid);
                !is_allowlisted && !is_denylisted
            })
            .collect()
    }

    fn refresh_menu_dropdown<F>(
        menu: &ViewHandle<FilterableDropdown<AISettingsPageAction>>,
        action_fn: F,
        ctx: &mut ViewContext<Self>,
    ) where
        F: Fn(uuid::Uuid) -> AISettingsPageAction,
    {
        let mcps_in_dropdown = Self::get_non_allowlisted_or_denylisted_mcp_servers(ctx);
        menu.update(ctx, |menu, ctx| {
            if AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                menu.set_enabled(ctx);
            } else {
                menu.set_disabled(ctx);
            }

            let items: Vec<DropdownItem<AISettingsPageAction>> = mcps_in_dropdown
                .iter()
                .map(|(uuid, server_name)| DropdownItem::new(server_name, action_fn(*uuid)))
                .collect();

            menu.set_items(items, ctx);
            ctx.notify()
        });
        ctx.notify();
    }

    fn refresh_mcp_allowlist_dropdown(
        menu: &ViewHandle<FilterableDropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        Self::refresh_menu_dropdown(menu, AISettingsPageAction::AddToMCPAllowlist, ctx);
    }

    fn create_profile_views(ctx: &mut ViewContext<Self>) -> Vec<ViewHandle<ExecutionProfileView>> {
        let profiles_model = AIExecutionProfilesModel::as_ref(ctx);
        let profile_ids = profiles_model.get_all_profile_ids();

        profile_ids
            .iter()
            .map(|&profile_id| {
                let profile_view =
                    ctx.add_typed_action_view(|ctx| ExecutionProfileView::new(profile_id, ctx));

                ctx.subscribe_to_view(&profile_view, move |_me, _, event, ctx| match event {
                    ExecutionProfileViewEvent::EditProfile => {
                        ctx.emit(AISettingsPageEvent::OpenExecutionProfileEditor(profile_id));
                    }
                });

                profile_view
            })
            .collect()
    }

    fn refresh_profile_views(&mut self, ctx: &mut ViewContext<Self>) {
        let new_profile_views = Self::create_profile_views(ctx);
        self.profile_views = new_profile_views;
    }

    fn refresh_mcp_denylist_dropdown(
        menu: &ViewHandle<FilterableDropdown<AISettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        Self::refresh_menu_dropdown(menu, AISettingsPageAction::AddToMCPDenylist, ctx);
    }

    fn create_cli_agent_builtin_prompt_editors(
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<EditorView>> {
        AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .map(|agent| {
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
                    EditorEvent::Escape => ctx.emit(AISettingsPageEvent::FocusModal),
                    _ => {}
                });

                editor
            })
            .collect()
    }

    fn create_cli_agent_builtin_prompt_mode_dropdowns(
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<Dropdown<AISettingsPageAction>>> {
        AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .map(|agent| {
                ctx.add_typed_action_view(move |ctx| {
                    let mut dropdown = Dropdown::new(ctx);
                    dropdown.set_top_bar_max_width(160.);
                    dropdown.set_menu_width(180., ctx);
                    dropdown.set_main_axis_size(MainAxisSize::Min, ctx);

                    dropdown.add_items(
                        CLIAgentBuiltinPromptMode::iter()
                            .map(|mode| {
                                DropdownItem::new(
                                    mode.display_name(),
                                    AISettingsPageAction::SetCLIAgentBuiltinPromptMode {
                                        agent,
                                        mode,
                                    },
                                )
                            })
                            .collect(),
                        ctx,
                    );

                    let current_mode = AISettings::as_ref(ctx).cli_agent_builtin_prompt(agent).mode;
                    dropdown.set_selected_by_action(
                        AISettingsPageAction::SetCLIAgentBuiltinPromptMode {
                            agent,
                            mode: current_mode,
                        },
                        ctx,
                    );

                    dropdown
                })
            })
            .collect()
    }

    fn sync_cli_agent_builtin_prompt_editors(&mut self, ctx: &mut ViewContext<Self>) {
        for (idx, agent) in AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .enumerate()
        {
            let Some(editor) = self.cli_agent_builtin_prompt_editors.get(idx) else {
                continue;
            };
            let prompt = AISettings::as_ref(ctx)
                .cli_agent_builtin_prompt(agent)
                .prompt;
            editor.update(ctx, |editor, ctx| {
                if editor.buffer_text(ctx) != prompt {
                    editor.system_reset_buffer_text(&prompt, ctx);
                }
            });
        }
    }

    fn refresh_cli_agent_builtin_prompt_mode_dropdowns(&mut self, ctx: &mut ViewContext<Self>) {
        for (idx, agent) in AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .enumerate()
        {
            let Some(dropdown) = self.cli_agent_builtin_prompt_mode_dropdowns.get(idx) else {
                continue;
            };
            let current_mode = AISettings::as_ref(ctx).cli_agent_builtin_prompt(agent).mode;
            dropdown.update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentBuiltinPromptMode {
                        agent,
                        mode: current_mode,
                    },
                    ctx,
                );
            });
        }
    }

    fn cli_agent_api_profile_preset(preset_id: &str) -> Option<&'static CLIAgentApiProfilePreset> {
        CLI_AGENT_API_PROFILE_PRESETS
            .iter()
            .find(|preset| preset.id == preset_id)
    }

    fn create_cli_agent_api_profile_preset_dropdown(
        selected_preset_id: &str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        let selected_preset_id = selected_preset_id.to_owned();
        ctx.add_typed_action_view(move |ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(220.);
            dropdown.set_menu_width(280., ctx);
            dropdown.set_main_axis_size(MainAxisSize::Min, ctx);
            let mut items = vec![DropdownItem::new(
                "Custom provider",
                AISettingsPageAction::SetCLIAgentApiProfilePreset(
                    CLI_AGENT_API_CUSTOM_PRESET_ID.to_owned(),
                ),
            )];
            items.extend(CLI_AGENT_API_PROFILE_PRESETS.iter().map(|preset| {
                DropdownItem::new(
                    preset.label,
                    AISettingsPageAction::SetCLIAgentApiProfilePreset(preset.id.to_owned()),
                )
            }));
            dropdown.set_items(items, ctx);
            dropdown.set_selected_by_action(
                AISettingsPageAction::SetCLIAgentApiProfilePreset(selected_preset_id.clone()),
                ctx,
            );
            dropdown
        })
    }

    fn create_cli_agent_api_profile_agent_dropdown(
        selected_agent: CLIAgent,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        ctx.add_typed_action_view(move |ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(190.);
            dropdown.set_menu_width(220., ctx);
            dropdown.set_main_axis_size(MainAxisSize::Min, ctx);
            dropdown.add_items(
                AISettings::cli_agent_api_profile_agents()
                    .into_iter()
                    .map(|agent| {
                        DropdownItem::new(
                            agent.display_name(),
                            AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(agent),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_action(
                AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(selected_agent),
                ctx,
            );
            dropdown
        })
    }

    fn create_cli_agent_api_profile_environment_dropdown(
        selected_environment_id: &str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        let selected_environment_id = selected_environment_id.to_owned();
        ctx.add_typed_action_view(move |ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(220.);
            dropdown.set_menu_width(260., ctx);
            dropdown.set_main_axis_size(MainAxisSize::Min, ctx);
            Self::populate_cli_agent_api_profile_environment_dropdown(
                &mut dropdown,
                &selected_environment_id,
                ctx,
            );
            dropdown
        })
    }

    fn populate_cli_agent_api_profile_environment_dropdown(
        dropdown: &mut Dropdown<AISettingsPageAction>,
        selected_environment_id: &str,
        ctx: &mut ViewContext<Dropdown<AISettingsPageAction>>,
    ) {
        let mut items = vec![
            DropdownItem::new(
                "Local",
                AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(
                    CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
                ),
            ),
            DropdownItem::new(
                "All environments",
                AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(
                    CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned(),
                ),
            ),
        ];

        for host in SshRemoteModel::as_ref(ctx).hosts() {
            items.push(DropdownItem::new(
                format!("SSH {}", host.display_name()),
                AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(
                    ssh_remote_environment_id(&host.id),
                ),
            ));
        }

        dropdown.set_items(items, ctx);
        dropdown.set_selected_by_action(
            AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(
                selected_environment_id.to_owned(),
            ),
            ctx,
        );
    }

    fn refresh_cli_agent_api_profile_environment_dropdown(&mut self, ctx: &mut ViewContext<Self>) {
        let selected_environment_id = if self
            .cli_agent_api_profile_draft_environment_id
            .starts_with("ssh:")
            && SshRemoteModel::as_ref(ctx).hosts().iter().all(|host| {
                ssh_remote_environment_id(&host.id)
                    != self.cli_agent_api_profile_draft_environment_id
            }) {
            self.cli_agent_api_profile_draft_environment_id =
                CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned();
            self.cli_agent_api_profile_draft_environment_id.clone()
        } else {
            self.cli_agent_api_profile_draft_environment_id.clone()
        };

        self.cli_agent_api_profile_environment_dropdown
            .update(ctx, |dropdown, ctx| {
                Self::populate_cli_agent_api_profile_environment_dropdown(
                    dropdown,
                    &selected_environment_id,
                    ctx,
                );
            });
    }

    fn cli_agent_api_profile_anthropic_base_url(profile: &CLIAgentApiProfile) -> String {
        if profile.agent() == CLIAgent::Claude {
            profile.base_url.clone()
        } else {
            profile
                .extra_env
                .get(CLI_AGENT_API_ANTHROPIC_BASE_URL_ENV_KEY)
                .cloned()
                .unwrap_or_default()
        }
    }

    fn cli_agent_api_profile_openai_base_url(profile: &CLIAgentApiProfile) -> String {
        if matches!(
            profile.agent(),
            CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Hermes
        ) {
            profile.base_url.clone()
        } else {
            profile
                .extra_env
                .get(CLI_AGENT_API_OPENAI_BASE_URL_ENV_KEY)
                .cloned()
                .unwrap_or_default()
        }
    }

    fn cli_agent_api_profile_base_url_for_agent(
        agent: CLIAgent,
        anthropic_base_url: &str,
        openai_base_url: &str,
    ) -> String {
        match agent {
            CLIAgent::Claude => anthropic_base_url.to_owned(),
            CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Hermes => openai_base_url.to_owned(),
            _ => openai_base_url
                .trim()
                .is_empty()
                .then(|| anthropic_base_url.to_owned())
                .unwrap_or_else(|| openai_base_url.to_owned()),
        }
    }

    fn create_cli_agent_api_profile_model_picker_dropdown(
        selected_model: &str,
        models: &[String],
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        let selected_model = selected_model.to_owned();
        let models = models.to_vec();
        ctx.add_typed_action_view(move |ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(260.);
            dropdown.set_menu_width(320., ctx);
            dropdown.set_main_axis_size(MainAxisSize::Min, ctx);
            Self::populate_cli_agent_api_profile_model_picker_dropdown(
                &mut dropdown,
                &selected_model,
                &models,
                ctx,
            );
            dropdown
        })
    }

    fn populate_cli_agent_api_profile_model_picker_dropdown(
        dropdown: &mut Dropdown<AISettingsPageAction>,
        selected_model: &str,
        models: &[String],
        ctx: &mut ViewContext<Dropdown<AISettingsPageAction>>,
    ) {
        let selected_model = if models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(selected_model))
        {
            selected_model.to_owned()
        } else {
            models.first().cloned().unwrap_or_default()
        };
        let items = if models.is_empty() {
            vec![DropdownItem::new(
                "暂无模型",
                AISettingsPageAction::SelectCLIAgentApiProfileFetchedModel(String::new()),
            )]
        } else {
            models
                .iter()
                .map(|model| {
                    DropdownItem::new(
                        model.clone(),
                        AISettingsPageAction::SelectCLIAgentApiProfileFetchedModel(model.clone()),
                    )
                })
                .collect()
        };

        dropdown.set_items(items, ctx);
        dropdown.set_selected_by_action(
            AISettingsPageAction::SelectCLIAgentApiProfileFetchedModel(selected_model),
            ctx,
        );
    }

    fn refresh_cli_agent_api_profile_model_picker_dropdown(&mut self, ctx: &mut ViewContext<Self>) {
        let selected_model = if self
            .cli_agent_api_profile_fetched_models
            .iter()
            .any(|model| {
                model.eq_ignore_ascii_case(&self.cli_agent_api_profile_selected_fetched_model)
            }) {
            self.cli_agent_api_profile_selected_fetched_model.clone()
        } else {
            self.cli_agent_api_profile_fetched_models
                .first()
                .cloned()
                .unwrap_or_default()
        };
        self.cli_agent_api_profile_selected_fetched_model = selected_model.clone();
        let models = self.cli_agent_api_profile_fetched_models.clone();
        self.cli_agent_api_profile_model_picker_dropdown
            .update(ctx, |dropdown, ctx| {
                Self::populate_cli_agent_api_profile_model_picker_dropdown(
                    dropdown,
                    &selected_model,
                    &models,
                    ctx,
                );
            });
    }

    fn create_cli_agent_api_profile_editor(
        placeholder: &'static str,
        is_password: bool,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(move |ctx| {
            let appearance = Appearance::handle(ctx).as_ref(ctx);
            let options = SingleLineEditorOptions {
                is_password,
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
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text(placeholder, ctx);
            editor
        });
        Self::update_editor_interaction_state(editor.clone(), true, ctx);
        editor
    }

    fn create_cli_agent_api_profiles_json_editor(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
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
                "Paste Agent API profiles JSON, or copy the current export here.",
                ctx,
            );
            editor
        });
        Self::update_editor_interaction_state(editor.clone(), true, ctx);
        editor
    }

    fn create_cli_agent_api_profile_multiline_editor(
        placeholder: &'static str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(move |ctx| {
            let appearance = Appearance::handle(ctx).as_ref(ctx);
            let options = EditorOptions {
                autogrow: true,
                soft_wrap: true,
                placeholder_soft_wrap: true,
                enter_settings: EnterSettings {
                    enter: EnterAction::InsertNewLineIfMultiLine,
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
            editor.set_placeholder_text(placeholder, ctx);
            editor
        });
        Self::update_editor_interaction_state(editor.clone(), true, ctx);
        editor
    }

    fn create_cli_agent_api_profile_extra_env_editor(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        Self::create_cli_agent_api_profile_multiline_editor(
            "KEY=value per line, e.g. header:HTTP-Referer=https://app.example",
            ctx,
        )
    }

    fn sync_cli_agent_api_profile_mouse_state_handles(&mut self, ctx: &mut ViewContext<Self>) {
        let profile_count = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .len();
        self.cli_agent_api_profile_mouse_state_handles =
            (0..profile_count).map(|_| Default::default()).collect();
        self.cli_agent_api_profile_edit_mouse_state_handles =
            (0..profile_count).map(|_| Default::default()).collect();
        self.cli_agent_api_profile_check_mouse_state_handles =
            (0..profile_count).map(|_| Default::default()).collect();
        self.cli_agent_api_profile_toggle_mouse_state_handles =
            (0..profile_count).map(|_| Default::default()).collect();
        self.cli_agent_api_profile_remove_mouse_state_handles =
            (0..profile_count).map(|_| Default::default()).collect();
    }

    fn set_cli_agent_api_profile_preset_selection(
        &mut self,
        preset_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        self.cli_agent_api_profile_selected_preset_id = preset_id.to_owned();
        self.cli_agent_api_profile_preset_dropdown
            .update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentApiProfilePreset(preset_id.to_owned()),
                    ctx,
                );
            });
    }

    fn handle_set_cli_agent_api_profile_preset(
        &mut self,
        preset_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        self.cli_agent_api_profile_selected_preset_id = preset_id.to_owned();
        let Some(preset) = Self::cli_agent_api_profile_preset(preset_id) else {
            ctx.notify();
            return;
        };

        self.cli_agent_api_profile_draft_agent = preset.agent;
        self.cli_agent_api_profile_agent_dropdown
            .update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(preset.agent),
                    ctx,
                );
            });
        let anthropic_base_url = if preset.agent == CLIAgent::Claude {
            preset.base_url
        } else {
            ""
        };
        let openai_base_url = if matches!(
            preset.agent,
            CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Hermes
        ) {
            preset.base_url
        } else {
            ""
        };

        for (editor, text) in [
            (&self.cli_agent_api_profile_name_editor, preset.profile_name),
            (
                &self.cli_agent_api_profile_base_url_editor,
                anthropic_base_url,
            ),
            (
                &self.cli_agent_api_profile_openai_base_url_editor,
                openai_base_url,
            ),
            (
                &self.cli_agent_api_profile_api_format_editor,
                Self::cli_agent_api_default_api_format(preset.agent),
            ),
            (
                &self.cli_agent_api_profile_auth_env_var_editor,
                Self::cli_agent_api_default_auth_env_var(preset.agent),
            ),
            (&self.cli_agent_api_profile_full_url_mode_editor, "false"),
            (&self.cli_agent_api_profile_model_editor, preset.model),
        ] {
            editor.update(ctx, |editor, ctx| editor.set_buffer_text(text, ctx));
        }
        self.cli_agent_api_profile_priority_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text(&preset.priority.to_string(), ctx);
            });
        for editor in [
            &self.cli_agent_api_profile_input_cost_editor,
            &self.cli_agent_api_profile_output_cost_editor,
        ] {
            editor.update(ctx, |editor, ctx| editor.set_buffer_text("0", ctx));
        }
        self.cli_agent_api_profile_extra_env_editor
            .update(ctx, |editor, ctx| editor.set_buffer_text("", ctx));
        self.cli_agent_api_profile_model_catalog_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text(preset.model, ctx);
            });
        self.cli_agent_api_profile_fetched_models = if preset.model.trim().is_empty() {
            Vec::new()
        } else {
            vec![preset.model.to_owned()]
        };
        self.cli_agent_api_profile_selected_fetched_model = preset.model.to_owned();
        self.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
        self.cli_agent_api_profile_model_mappings_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text(
                    &Self::default_cli_agent_api_model_mappings_text(preset.agent, preset.model),
                    ctx,
                );
            });
        ctx.notify();
    }

    fn cli_agent_api_default_api_format(agent: CLIAgent) -> &'static str {
        match agent {
            CLIAgent::Claude => "anthropic_messages",
            CLIAgent::Gemini => "gemini",
            CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Hermes => "openai_chat",
            _ => "openai_chat",
        }
    }

    fn cli_agent_api_default_auth_env_var(agent: CLIAgent) -> &'static str {
        match agent {
            CLIAgent::Claude => "ANTHROPIC_AUTH_TOKEN",
            CLIAgent::Gemini => "GEMINI_API_KEY",
            CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Hermes => "OPENAI_API_KEY",
            _ => "OPENAI_API_KEY",
        }
    }

    fn default_cli_agent_api_model_mappings_text(agent: CLIAgent, model: &str) -> String {
        let model = model.trim();
        if model.is_empty() {
            return String::new();
        }
        match agent {
            CLIAgent::Claude => ["Sonnet", "Opus", "Haiku"]
                .into_iter()
                .map(|role| format!("{role},{model},{model}"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => format!("Default,{model},{model}"),
        }
    }

    fn parse_cli_agent_api_cost_per_million_tokens(text: &str) -> f64 {
        let value = text.trim().parse::<f64>().unwrap_or_default();
        if value.is_finite() && value > 0.0 {
            value
        } else {
            0.0
        }
    }

    fn parse_cli_agent_api_extra_env(text: &str) -> HashMap<String, String> {
        text.lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let (key, value) = line.split_once('=')?;
                let key = key.trim();
                let value = value.trim();
                if key.is_empty() || value.is_empty() {
                    None
                } else {
                    Some((key.to_owned(), value.to_owned()))
                }
            })
            .collect()
    }

    fn format_cli_agent_api_extra_env(extra_env: &HashMap<String, String>) -> String {
        let mut entries = extra_env
            .iter()
            .filter(|(key, value)| !key.trim().is_empty() && !value.trim().is_empty())
            .map(|(key, value)| format!("{}={}", key.trim(), value.trim()))
            .collect::<Vec<_>>();
        entries.sort();
        entries.join("\n")
    }

    fn parse_cli_agent_api_model_catalog(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ToOwned::to_owned)
            .unique()
            .collect()
    }

    fn format_cli_agent_api_model_catalog(models: &[String]) -> String {
        models
            .iter()
            .map(|model| model.trim())
            .filter(|model| !model.is_empty())
            .unique()
            .join("\n")
    }

    fn parse_cli_agent_api_model_mappings(text: &str) -> Vec<CLIAgentApiModelMapping> {
        text.lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }

                let (role, display_name, model, context_window_tokens) =
                    if let Some((left, model)) = line.split_once("=>") {
                        let (role, display_name) = left
                            .split_once('=')
                            .map(|(role, display)| (role, display))
                            .unwrap_or((left, ""));
                        (role, display_name, model, 0)
                    } else {
                        let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
                        (
                            parts.first().copied().unwrap_or_default(),
                            parts.get(1).copied().unwrap_or_default(),
                            parts.get(2).copied().unwrap_or_default(),
                            parts
                                .get(3)
                                .and_then(|value| value.parse::<u32>().ok())
                                .unwrap_or_default(),
                        )
                    };

                let role = role.trim();
                let display_name = display_name.trim();
                let model = model.trim();
                if role.is_empty() && display_name.is_empty() && model.is_empty() {
                    return None;
                }
                Some(CLIAgentApiModelMapping {
                    role: role.to_owned(),
                    display_name: display_name.to_owned(),
                    model: model.to_owned(),
                    supports_one_million_context: context_window_tokens >= 1_000_000,
                    context_window_tokens,
                })
            })
            .collect()
    }

    fn format_cli_agent_api_model_mappings(mappings: &[CLIAgentApiModelMapping]) -> String {
        mappings
            .iter()
            .map(|mapping| {
                let context = if mapping.context_window_tokens > 0 {
                    format!(",{}", mapping.context_window_tokens)
                } else if mapping.supports_one_million_context {
                    ",1000000".to_owned()
                } else {
                    String::new()
                };
                format!(
                    "{},{},{}{}",
                    mapping.role.trim(),
                    mapping.display_name.trim(),
                    mapping.model.trim(),
                    context
                )
            })
            .filter(|line| line.split(',').any(|part| !part.trim().is_empty()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn cli_agent_api_profile_model_catalog(&self, app: &AppContext) -> Vec<String> {
        Self::parse_cli_agent_api_model_catalog(
            &self
                .cli_agent_api_profile_model_catalog_editor
                .as_ref(app)
                .buffer_text(app),
        )
    }

    fn set_cli_agent_api_profile_model_catalog(
        &self,
        models: &[String],
        ctx: &mut ViewContext<Self>,
    ) {
        self.cli_agent_api_profile_model_catalog_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text(&Self::format_cli_agent_api_model_catalog(models), ctx);
            });
    }

    fn clear_cli_agent_api_profile_save_feedback(&mut self, ctx: &mut ViewContext<Self>) {
        self.cli_agent_api_profile_save_feedback = None;
        self.cli_agent_api_profile_save_feedback_is_error = false;
        self.cli_agent_api_profile_save_feedback_generation = self
            .cli_agent_api_profile_save_feedback_generation
            .wrapping_add(1);
        self.cli_agent_api_profile_add_button
            .update(ctx, |button, ctx| {
                button.set_label("保存", ctx);
                button.set_icon(Some(Icon::Check), ctx);
            });
    }

    fn sync_cli_agent_api_profile_add_model_button(&mut self, ctx: &mut ViewContext<Self>) {
        let open = self.cli_agent_api_profile_add_model_open;
        self.cli_agent_api_profile_toggle_add_model_button
            .update(ctx, |button, ctx| {
                button.set_label(if open { "收起" } else { "添加模型" }, ctx);
                button.set_icon(Some(if open { Icon::X } else { Icon::Plus }), ctx);
            });
    }

    fn set_cli_agent_api_profile_save_feedback(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let message = message.into();
        self.cli_agent_api_profile_save_feedback = Some(message.clone());
        self.cli_agent_api_profile_save_feedback_is_error = is_error;
        self.cli_agent_api_profile_save_feedback_generation = self
            .cli_agent_api_profile_save_feedback_generation
            .wrapping_add(1);
        let generation = self.cli_agent_api_profile_save_feedback_generation;
        self.cli_agent_api_profile_add_button
            .update(ctx, |button, ctx| {
                button.set_label(if is_error { "保存" } else { "已保存" }, ctx);
                button.set_icon(Some(Icon::Check), ctx);
            });

        let window_id = ctx.window_id();
        crate::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            let toast = if is_error {
                crate::view_components::DismissibleToast::error(message.clone())
            } else {
                crate::view_components::DismissibleToast::success(message.clone())
            };
            toast_stack.add_ephemeral_toast(toast, window_id, ctx);
        });

        let timeout = if is_error {
            Duration::from_millis(2600)
        } else {
            Duration::from_millis(1800)
        };
        ctx.spawn(Timer::after(timeout), move |me, _, ctx| {
            if me.cli_agent_api_profile_save_feedback_generation != generation {
                return;
            }
            me.cli_agent_api_profile_save_feedback = None;
            me.cli_agent_api_profile_save_feedback_is_error = false;
            me.cli_agent_api_profile_add_button
                .update(ctx, |button, ctx| {
                    button.set_label("保存", ctx);
                    button.set_icon(Some(Icon::Check), ctx);
                });
            ctx.notify();
        });
        ctx.notify();
    }

    fn cli_agent_api_claude_settings_env_vars(
        env_vars: HashMap<String, String>,
    ) -> HashMap<String, String> {
        env_vars
            .into_iter()
            .filter(|(key, value)| {
                CLI_AGENT_API_CLAUDE_SETTINGS_ENV_KEYS.contains(&key.as_str())
                    && !value.trim().is_empty()
            })
            .collect()
    }

    fn has_cli_agent_api_claude_settings_env_vars(env_vars: &HashMap<String, String>) -> bool {
        env_vars.iter().any(|(key, value)| {
            CLI_AGENT_API_CLAUDE_SETTINGS_ENV_KEYS.contains(&key.as_str())
                && !value.trim().is_empty()
        })
    }

    fn write_local_claude_agent_api_settings_blocking(
        env_vars: HashMap<String, String>,
    ) -> Result<(), String> {
        let env_vars = Self::cli_agent_api_claude_settings_env_vars(env_vars);
        if env_vars.is_empty() {
            return Ok(());
        }

        let home_dir =
            dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_owned())?;
        let claude_dir = home_dir.join(".claude");
        let settings_path = claude_dir.join("settings.json");
        let mut settings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !settings.is_object() {
            settings = serde_json::json!({});
        }

        let root = settings
            .as_object_mut()
            .ok_or_else(|| "Claude settings root is not an object".to_owned())?;
        root.insert("skipIntroduction".to_owned(), serde_json::Value::Bool(true));
        root.insert(
            "skipDangerousModePermissionPrompt".to_owned(),
            serde_json::Value::Bool(true),
        );

        let env = root
            .entry("env".to_owned())
            .or_insert_with(|| serde_json::json!({}));
        if !env.is_object() {
            *env = serde_json::json!({});
        }
        let env = env
            .as_object_mut()
            .ok_or_else(|| "Claude settings env is not an object".to_owned())?;
        for (key, value) in env_vars {
            env.insert(key, serde_json::Value::String(value));
        }

        fs::create_dir_all(&claude_dir)
            .map_err(|error| format!("Could not create {}: {error}", claude_dir.display()))?;
        let mut serialized = serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("Could not serialize Claude settings: {error}"))?;
        serialized.push(b'\n');
        fs::write(&settings_path, serialized)
            .map_err(|error| format!("Could not write {}: {error}", settings_path.display()))
    }

    async fn sync_local_claude_agent_api_settings(
        env_vars: HashMap<String, String>,
    ) -> Result<(), String> {
        tokio::task::spawn_blocking(move || {
            Self::write_local_claude_agent_api_settings_blocking(env_vars)
        })
        .await
        .map_err(|error| format!("Local Claude settings sync task failed: {error}"))?
    }

    fn cli_agent_api_codex_settings_env_vars(
        env_vars: HashMap<String, String>,
    ) -> HashMap<String, String> {
        env_vars
            .into_iter()
            .filter(|(key, value)| {
                CLI_AGENT_API_CODEX_SETTINGS_ENV_KEYS.contains(&key.as_str())
                    && !value.trim().is_empty()
            })
            .collect()
    }

    fn has_cli_agent_api_codex_settings_env_vars(env_vars: &HashMap<String, String>) -> bool {
        env_vars.iter().any(|(key, value)| {
            CLI_AGENT_API_CODEX_SETTINGS_ENV_KEYS.contains(&key.as_str())
                && !value.trim().is_empty()
        })
    }

    fn toml_basic_string(value: &str) -> Result<String, String> {
        serde_json::to_string(value)
            .map_err(|error| format!("Could not serialize TOML string: {error}"))
    }

    fn is_toml_section_line(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with('[') && trimmed.ends_with(']')
    }

    fn is_toml_assignment_for_key(line: &str, key: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with(key) && trimmed[key.len()..].trim_start().starts_with('=')
    }

    fn finish_toml_lines(mut lines: Vec<String>) -> String {
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        }
    }

    fn upsert_toml_top_level_key(content: &str, key: &str, value: &str) -> String {
        let mut lines = content.lines().map(str::to_owned).collect::<Vec<String>>();
        let first_section = lines
            .iter()
            .position(|line| Self::is_toml_section_line(line));
        let limit = first_section.unwrap_or(lines.len());
        let assignment = format!("{key} = {value}");

        for line in lines.iter_mut().take(limit) {
            if Self::is_toml_assignment_for_key(line, key) {
                *line = assignment;
                return Self::finish_toml_lines(lines);
            }
        }

        if let Some(index) = first_section {
            lines.insert(index, assignment);
        } else {
            lines.push(assignment);
        }
        Self::finish_toml_lines(lines)
    }

    fn upsert_toml_section_key(content: &str, section: &str, key: &str, value: &str) -> String {
        let mut lines = content.lines().map(str::to_owned).collect::<Vec<String>>();
        let assignment = format!("{key} = {value}");
        let Some(start) = lines.iter().position(|line| line.trim() == section) else {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(section.to_owned());
            lines.push(assignment);
            return Self::finish_toml_lines(lines);
        };

        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(index, line)| Self::is_toml_section_line(line).then_some(index))
            .unwrap_or(lines.len());

        for line in lines.iter_mut().take(end).skip(start + 1) {
            if Self::is_toml_assignment_for_key(line, key) {
                *line = assignment;
                return Self::finish_toml_lines(lines);
            }
        }

        lines.insert(end, assignment);
        Self::finish_toml_lines(lines)
    }

    fn write_local_codex_auth_json(
        auth_path: &Path,
        auth: &serde_json::Value,
    ) -> Result<(), String> {
        let serialized = serde_json::to_vec_pretty(auth)
            .map_err(|error| format!("Could not serialize Codex auth.json: {error}"))?;
        if let Some(parent) = auth_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }

        #[cfg(unix)]
        {
            use std::io::Write as _;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(auth_path)
                .map_err(|error| format!("Could not open {}: {error}", auth_path.display()))?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| {
                    format!(
                        "Could not set permissions on {}: {error}",
                        auth_path.display()
                    )
                })?;
            file.write_all(&serialized)
                .map_err(|error| format!("Could not write {}: {error}", auth_path.display()))?;
            file.write_all(b"\n")
                .map_err(|error| format!("Could not write {}: {error}", auth_path.display()))?;
        }

        #[cfg(not(unix))]
        {
            let mut serialized = serialized;
            serialized.push(b'\n');
            fs::write(auth_path, serialized)
                .map_err(|error| format!("Could not write {}: {error}", auth_path.display()))?;
        }

        Ok(())
    }

    fn write_local_codex_agent_api_settings_blocking(
        env_vars: HashMap<String, String>,
    ) -> Result<(), String> {
        let env_vars = Self::cli_agent_api_codex_settings_env_vars(env_vars);
        if env_vars.is_empty() {
            return Ok(());
        }

        let home_dir =
            dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_owned())?;
        let codex_dir = home_dir.join(".codex");

        if let Some(api_key) = env_vars
            .get("OPENAI_API_KEY")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let auth_path = codex_dir.join("auth.json");
            let mut auth = fs::read_to_string(&auth_path)
                .ok()
                .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if !auth.is_object() {
                auth = serde_json::json!({});
            }
            let root = auth
                .as_object_mut()
                .ok_or_else(|| "Codex auth root is not an object".to_owned())?;
            root.insert(
                "OPENAI_API_KEY".to_owned(),
                serde_json::Value::String(api_key.to_owned()),
            );
            root.entry("auth_mode".to_owned())
                .or_insert_with(|| serde_json::Value::String("apikey".to_owned()));
            Self::write_local_codex_auth_json(&auth_path, &auth)?;
        }

        let config_path = codex_dir.join("config.toml");
        let mut config = fs::read_to_string(&config_path).unwrap_or_default();
        if let Some(base_url) = env_vars
            .get("OPENAI_BASE_URL")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            config = Self::upsert_toml_top_level_key(
                &config,
                "openai_base_url",
                &Self::toml_basic_string(base_url)?,
            );
        }
        config = Self::upsert_toml_top_level_key(&config, "check_for_update_on_startup", "false");
        if let Some(model) = env_vars
            .get("OPENAI_MODEL")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && *value != "default")
        {
            config =
                Self::upsert_toml_top_level_key(&config, "model", &Self::toml_basic_string(model)?);
            config = Self::upsert_toml_section_key(
                &config,
                "[notice.model_migrations]",
                &Self::toml_basic_string(model)?,
                &Self::toml_basic_string(CLI_AGENT_API_CODEX_MODEL_MIGRATION_TARGET)?,
            );
        }

        fs::create_dir_all(&codex_dir)
            .map_err(|error| format!("Could not create {}: {error}", codex_dir.display()))?;
        fs::write(&config_path, config)
            .map_err(|error| format!("Could not write {}: {error}", config_path.display()))
    }

    async fn sync_local_codex_agent_api_settings(
        env_vars: HashMap<String, String>,
    ) -> Result<(), String> {
        tokio::task::spawn_blocking(move || {
            Self::write_local_codex_agent_api_settings_blocking(env_vars)
        })
        .await
        .map_err(|error| format!("Local Codex settings sync task failed: {error}"))?
    }

    fn handle_cli_agent_api_settings_sync_result(
        &mut self,
        feedback_prefix: &str,
        target: &str,
        result: Result<(), String>,
        ctx: &mut ViewContext<Self>,
    ) {
        match result {
            Ok(()) => {
                self.set_cli_agent_api_profile_save_feedback(
                    format!("{feedback_prefix}，{target} 已同步"),
                    false,
                    ctx,
                );
            }
            Err(error) => {
                log::warn!("Failed to sync Agent API settings for {target}: {error}");
                self.set_cli_agent_api_profile_save_feedback(
                    format!("{feedback_prefix}，{target} 同步失败"),
                    true,
                    ctx,
                );
            }
        }
    }

    fn sync_cli_agent_api_profile_settings(
        &mut self,
        profile_id: &str,
        feedback_prefix: impl Into<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let feedback_prefix = feedback_prefix.into();
        let Some(profile) = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
        else {
            return;
        };
        let profile_agent = profile.agent();
        if !profile.enabled {
            return;
        }
        let profile_env_vars = AISettings::cli_agent_api_profile_native_environment_vars(&profile);

        if profile.environment_id == CLI_AGENT_API_LOCAL_ENVIRONMENT_ID
            || profile.is_scoped_to_all_environments()
        {
            let env_vars = profile_env_vars.clone();
            match profile_agent {
                CLIAgent::Claude if Self::has_cli_agent_api_claude_settings_env_vars(&env_vars) => {
                    let feedback_prefix = feedback_prefix.clone();
                    ctx.spawn(
                        Self::sync_local_claude_agent_api_settings(env_vars),
                        move |me, result, ctx| {
                            me.handle_cli_agent_api_settings_sync_result(
                                &feedback_prefix,
                                "本地 Claude",
                                result,
                                ctx,
                            );
                        },
                    );
                }
                CLIAgent::Codex if Self::has_cli_agent_api_codex_settings_env_vars(&env_vars) => {
                    let feedback_prefix = feedback_prefix.clone();
                    ctx.spawn(
                        Self::sync_local_codex_agent_api_settings(env_vars),
                        move |me, result, ctx| {
                            me.handle_cli_agent_api_settings_sync_result(
                                &feedback_prefix,
                                "本地 Codex",
                                result,
                                ctx,
                            );
                        },
                    );
                }
                _ => {}
            }
        }

        let target_hosts = if let Some(host_id) = profile.environment_id.strip_prefix("ssh:") {
            SshRemoteModel::as_ref(ctx)
                .hosts()
                .iter()
                .find(|host| host.id == host_id)
                .cloned()
                .into_iter()
                .collect::<Vec<_>>()
        } else if profile.is_scoped_to_all_environments() {
            SshRemoteModel::as_ref(ctx)
                .active_host()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if profile.environment_id.starts_with("ssh:") && target_hosts.is_empty() {
            self.set_cli_agent_api_profile_save_feedback(
                format!("{feedback_prefix}，未找到对应 SSH remote"),
                true,
                ctx,
            );
            return;
        }

        for host in target_hosts {
            let env_vars = profile_env_vars.clone();
            let target_label = format!("SSH {}", host.display_name());
            match profile_agent {
                CLIAgent::Claude if Self::has_cli_agent_api_claude_settings_env_vars(&env_vars) => {
                    let feedback_prefix = feedback_prefix.clone();
                    ctx.spawn(
                        sync_remote_claude_agent_api_settings(host, env_vars),
                        move |me, result, ctx| {
                            me.handle_cli_agent_api_settings_sync_result(
                                &feedback_prefix,
                                &target_label,
                                result,
                                ctx,
                            );
                        },
                    );
                }
                CLIAgent::Codex if Self::has_cli_agent_api_codex_settings_env_vars(&env_vars) => {
                    let feedback_prefix = feedback_prefix.clone();
                    ctx.spawn(
                        sync_remote_codex_agent_api_settings(host, env_vars),
                        move |me, result, ctx| {
                            me.handle_cli_agent_api_settings_sync_result(
                                &feedback_prefix,
                                &target_label,
                                result,
                                ctx,
                            );
                        },
                    );
                }
                _ => {}
            }
        }
    }

    fn sync_saved_cli_agent_api_profile_settings(
        &mut self,
        profile_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        self.sync_cli_agent_api_profile_settings(profile_id, "供应商已保存", ctx);
    }

    fn sync_active_cli_agent_api_profile_settings(
        &mut self,
        agent: CLIAgent,
        environment_id: &str,
        feedback_prefix: impl Into<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(profile_id) = AISettings::as_ref(ctx)
            .active_cli_agent_api_profile(agent, environment_id)
            .map(|profile| profile.id.clone())
        else {
            return;
        };
        self.sync_cli_agent_api_profile_settings(&profile_id, feedback_prefix, ctx);
    }

    fn handle_add_cli_agent_api_profile_model(
        &mut self,
        model: &str,
        make_preferred: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let model = model.trim();
        if model.is_empty() {
            return;
        }

        let model = model.to_owned();
        let mut models = self.cli_agent_api_profile_model_catalog(ctx);
        if !models
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&model))
        {
            models.push(model.clone());
            self.set_cli_agent_api_profile_model_catalog(&models, ctx);
        }

        let current_model = self
            .cli_agent_api_profile_model_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        if make_preferred || current_model.trim().is_empty() {
            self.cli_agent_api_profile_model_editor
                .update(ctx, |editor, ctx| editor.set_buffer_text(&model, ctx));
        }

        let mapping_text = self
            .cli_agent_api_profile_model_mappings_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        if make_preferred || mapping_text.trim().is_empty() {
            self.cli_agent_api_profile_model_mappings_editor
                .update(ctx, |editor, ctx| {
                    editor.set_buffer_text(
                        &Self::default_cli_agent_api_model_mappings_text(
                            self.cli_agent_api_profile_draft_agent,
                            &model,
                        ),
                        ctx,
                    );
                });
        }

        self.cli_agent_api_profile_add_model_open = false;
        ctx.notify();
    }

    fn handle_remove_cli_agent_api_profile_model(
        &mut self,
        model: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let model = model.trim();
        if model.is_empty() {
            return;
        }

        let mut models = self.cli_agent_api_profile_model_catalog(ctx);
        models.retain(|existing| !existing.eq_ignore_ascii_case(model));
        self.set_cli_agent_api_profile_model_catalog(&models, ctx);

        let current_model = self
            .cli_agent_api_profile_model_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let mut preferred_model_was_removed = false;
        if current_model.trim().eq_ignore_ascii_case(model) {
            preferred_model_was_removed = true;
            let next_model = models.first().cloned().unwrap_or_default();
            self.cli_agent_api_profile_model_editor
                .update(ctx, |editor, ctx| editor.set_buffer_text(&next_model, ctx));
        }

        let mappings = Self::parse_cli_agent_api_model_mappings(
            &self
                .cli_agent_api_profile_model_mappings_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        let filtered_mappings = mappings
            .into_iter()
            .filter(|mapping| !mapping.model.eq_ignore_ascii_case(model))
            .collect::<Vec<_>>();
        let next_model = self
            .cli_agent_api_profile_model_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let next_mapping_text = if filtered_mappings.is_empty()
            && preferred_model_was_removed
            && !next_model.trim().is_empty()
        {
            Self::default_cli_agent_api_model_mappings_text(
                self.cli_agent_api_profile_draft_agent,
                &next_model,
            )
        } else {
            Self::format_cli_agent_api_model_mappings(&filtered_mappings)
        };
        self.cli_agent_api_profile_model_mappings_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text(&next_mapping_text, ctx);
            });

        ctx.notify();
    }

    fn reset_cli_agent_api_profile_form(&mut self, ctx: &mut ViewContext<Self>) {
        self.cli_agent_api_profile_editing_profile_id = None;
        self.cli_agent_api_profile_fetched_models.clear();
        self.cli_agent_api_profile_selected_fetched_model.clear();
        self.cli_agent_api_profile_fetch_models_error = None;
        self.cli_agent_api_profile_fetching_models = false;
        self.cli_agent_api_profile_add_model_open = false;
        self.cli_agent_api_profile_model_mapping_open = false;
        self.cli_agent_api_profile_fetch_models_request_id = self
            .cli_agent_api_profile_fetch_models_request_id
            .wrapping_add(1);
        self.sync_cli_agent_api_profile_add_model_button(ctx);
        self.clear_cli_agent_api_profile_save_feedback(ctx);
        self.set_cli_agent_api_profile_preset_selection(CLI_AGENT_API_CUSTOM_PRESET_ID, ctx);
        self.cli_agent_api_profile_draft_agent = CLIAgent::Claude;
        self.cli_agent_api_profile_draft_environment_id =
            CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned();
        self.cli_agent_api_profile_agent_dropdown
            .update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(CLIAgent::Claude),
                    ctx,
                );
            });
        self.cli_agent_api_profile_environment_dropdown
            .update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(
                        CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned(),
                    ),
                    ctx,
                );
            });
        self.cli_agent_api_profile_add_button
            .update(ctx, |button, ctx| {
                button.set_label("保存", ctx);
                button.set_icon(Some(Icon::Check), ctx);
            });
        for editor in [
            &self.cli_agent_api_profile_name_editor,
            &self.cli_agent_api_profile_base_url_editor,
            &self.cli_agent_api_profile_openai_base_url_editor,
            &self.cli_agent_api_profile_api_format_editor,
            &self.cli_agent_api_profile_auth_env_var_editor,
            &self.cli_agent_api_profile_full_url_mode_editor,
            &self.cli_agent_api_profile_api_key_editor,
            &self.cli_agent_api_profile_model_editor,
            &self.cli_agent_api_profile_model_catalog_editor,
            &self.cli_agent_api_profile_model_mappings_editor,
            &self.cli_agent_api_profile_priority_editor,
            &self.cli_agent_api_profile_input_cost_editor,
            &self.cli_agent_api_profile_output_cost_editor,
            &self.cli_agent_api_profile_extra_env_editor,
        ] {
            editor.update(ctx, |editor, ctx| editor.set_buffer_text("", ctx));
        }
        self.cli_agent_api_profile_api_format_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text("anthropic_messages", ctx);
            });
        self.cli_agent_api_profile_auth_env_var_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text("ANTHROPIC_AUTH_TOKEN", ctx);
            });
        self.cli_agent_api_profile_full_url_mode_editor
            .update(ctx, |editor, ctx| {
                editor.set_buffer_text("false", ctx);
            });
        self.cli_agent_api_profile_fetch_models_button
            .update(ctx, |button, ctx| {
                button.set_label("获取模型", ctx);
                button.set_disabled(false, ctx);
            });
        self.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
    }

    fn handle_edit_cli_agent_api_profile(&mut self, profile_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(profile) = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
        else {
            return;
        };

        let agent = profile.agent();
        let environment_id = profile.environment_id.clone();
        let profile_id = profile.id.clone();
        let should_sync_active_profile = profile.enabled && agent == CLIAgent::Claude;
        self.cli_agent_api_profile_fetched_models = profile.model_catalog.clone();
        self.cli_agent_api_profile_selected_fetched_model = profile
            .preferred_model()
            .trim()
            .is_empty()
            .then(|| {
                self.cli_agent_api_profile_fetched_models
                    .first()
                    .cloned()
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| profile.preferred_model());
        self.cli_agent_api_profile_fetch_models_error = None;
        self.cli_agent_api_profile_fetching_models = false;
        self.cli_agent_api_profile_add_model_open = false;
        self.cli_agent_api_profile_model_mapping_open = false;
        self.cli_agent_api_profile_fetch_models_request_id = self
            .cli_agent_api_profile_fetch_models_request_id
            .wrapping_add(1);
        self.sync_cli_agent_api_profile_add_model_button(ctx);
        self.clear_cli_agent_api_profile_save_feedback(ctx);
        self.cli_agent_api_profile_editing_profile_id = Some(profile.id.clone());
        self.set_cli_agent_api_profile_preset_selection(CLI_AGENT_API_CUSTOM_PRESET_ID, ctx);
        self.cli_agent_api_profile_draft_agent = agent;
        self.cli_agent_api_profile_draft_environment_id = environment_id.clone();
        self.cli_agent_api_profile_agent_dropdown
            .update(ctx, |dropdown, ctx| {
                dropdown.set_selected_by_action(
                    AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(agent),
                    ctx,
                );
            });
        self.refresh_cli_agent_api_profile_environment_dropdown(ctx);
        self.cli_agent_api_profile_add_button
            .update(ctx, |button, ctx| {
                button.set_label("保存", ctx);
                button.set_icon(Some(Icon::Check), ctx);
            });
        self.cli_agent_api_profile_fetch_models_button
            .update(ctx, |button, ctx| {
                button.set_label("获取模型", ctx);
                button.set_disabled(false, ctx);
            });

        let anthropic_base_url = Self::cli_agent_api_profile_anthropic_base_url(&profile);
        let openai_base_url = Self::cli_agent_api_profile_openai_base_url(&profile);
        for (editor, text) in [
            (&self.cli_agent_api_profile_name_editor, profile.name),
            (
                &self.cli_agent_api_profile_base_url_editor,
                anthropic_base_url,
            ),
            (
                &self.cli_agent_api_profile_openai_base_url_editor,
                openai_base_url,
            ),
            (
                &self.cli_agent_api_profile_api_format_editor,
                profile.api_format,
            ),
            (
                &self.cli_agent_api_profile_auth_env_var_editor,
                profile.auth_env_var,
            ),
            (
                &self.cli_agent_api_profile_full_url_mode_editor,
                profile.full_url_mode.to_string(),
            ),
            (&self.cli_agent_api_profile_api_key_editor, profile.api_key),
            (&self.cli_agent_api_profile_model_editor, profile.model),
            (
                &self.cli_agent_api_profile_model_catalog_editor,
                Self::format_cli_agent_api_model_catalog(&profile.model_catalog),
            ),
            (
                &self.cli_agent_api_profile_model_mappings_editor,
                Self::format_cli_agent_api_model_mappings(&profile.model_mappings),
            ),
            (
                &self.cli_agent_api_profile_priority_editor,
                profile.priority.to_string(),
            ),
            (
                &self.cli_agent_api_profile_input_cost_editor,
                profile.input_cost_per_million_tokens.to_string(),
            ),
            (
                &self.cli_agent_api_profile_output_cost_editor,
                profile.output_cost_per_million_tokens.to_string(),
            ),
            (
                &self.cli_agent_api_profile_extra_env_editor,
                Self::format_cli_agent_api_extra_env(&profile.extra_env),
            ),
        ] {
            editor.update(ctx, |editor, ctx| editor.set_buffer_text(&text, ctx));
        }

        if profile.enabled {
            let profile_id = profile_id.clone();
            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                settings.set_active_cli_agent_api_profile(agent, &environment_id, &profile_id, ctx);
            });
        }
        self.cli_agent_api_profile_editor_open = false;
        self.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
        if should_sync_active_profile {
            self.sync_cli_agent_api_profile_settings(&profile_id, "供应商已切换", ctx);
        }
        ctx.notify();
    }

    fn handle_add_cli_agent_api_profile(&mut self, ctx: &mut ViewContext<Self>) {
        let name = self
            .cli_agent_api_profile_name_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let anthropic_base_url = self
            .cli_agent_api_profile_base_url_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let openai_base_url = self
            .cli_agent_api_profile_openai_base_url_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let base_url = Self::cli_agent_api_profile_base_url_for_agent(
            self.cli_agent_api_profile_draft_agent,
            &anthropic_base_url,
            &openai_base_url,
        );
        let api_format = self
            .cli_agent_api_profile_api_format_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let auth_env_var = self
            .cli_agent_api_profile_auth_env_var_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let full_url_mode = matches!(
            self.cli_agent_api_profile_full_url_mode_editor
                .as_ref(ctx)
                .buffer_text(ctx)
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on" | "full"
        );
        let api_key = self
            .cli_agent_api_profile_api_key_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let model = self
            .cli_agent_api_profile_model_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let priority = self
            .cli_agent_api_profile_priority_editor
            .as_ref(ctx)
            .buffer_text(ctx)
            .trim()
            .parse::<u32>()
            .unwrap_or_default();
        let input_cost_per_million_tokens = Self::parse_cli_agent_api_cost_per_million_tokens(
            &self
                .cli_agent_api_profile_input_cost_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        let output_cost_per_million_tokens = Self::parse_cli_agent_api_cost_per_million_tokens(
            &self
                .cli_agent_api_profile_output_cost_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        let mut extra_env = Self::parse_cli_agent_api_extra_env(
            &self
                .cli_agent_api_profile_extra_env_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        extra_env.remove(CLI_AGENT_API_ANTHROPIC_BASE_URL_ENV_KEY);
        extra_env.remove(CLI_AGENT_API_OPENAI_BASE_URL_ENV_KEY);
        if !anthropic_base_url.trim().is_empty() {
            extra_env.insert(
                CLI_AGENT_API_ANTHROPIC_BASE_URL_ENV_KEY.to_owned(),
                anthropic_base_url.trim().to_owned(),
            );
        }
        if !openai_base_url.trim().is_empty() {
            extra_env.insert(
                CLI_AGENT_API_OPENAI_BASE_URL_ENV_KEY.to_owned(),
                openai_base_url.trim().to_owned(),
            );
        }
        let model_catalog = Self::parse_cli_agent_api_model_catalog(
            &self
                .cli_agent_api_profile_model_catalog_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        let mut model_mappings = Self::parse_cli_agent_api_model_mappings(
            &self
                .cli_agent_api_profile_model_mappings_editor
                .as_ref(ctx)
                .buffer_text(ctx),
        );
        if model_mappings.is_empty() && !model.trim().is_empty() {
            model_mappings = Self::parse_cli_agent_api_model_mappings(
                &Self::default_cli_agent_api_model_mappings_text(
                    self.cli_agent_api_profile_draft_agent,
                    &model,
                ),
            );
        }

        if base_url.trim().is_empty()
            && api_key.trim().is_empty()
            && model.trim().is_empty()
            && model_catalog.is_empty()
            && model_mappings.is_empty()
            && extra_env.is_empty()
        {
            log::warn!("Ignoring empty CLI agent API profile");
            self.set_cli_agent_api_profile_save_feedback("请先填写供应商信息再保存", true, ctx);
            return;
        }

        let mut profile = CLIAgentApiProfile::new(
            self.cli_agent_api_profile_draft_agent,
            self.cli_agent_api_profile_draft_environment_id.clone(),
            name,
            base_url,
            api_key,
            model,
        );
        profile.api_format = api_format;
        profile.full_url_mode = full_url_mode;
        profile.auth_env_var = auth_env_var;
        profile.model_catalog = model_catalog;
        profile.model_mappings = model_mappings;
        profile.priority = priority;
        profile.input_cost_per_million_tokens = input_cost_per_million_tokens;
        profile.output_cost_per_million_tokens = output_cost_per_million_tokens;
        profile.extra_env = extra_env;
        if let Some(profile_id) = self.cli_agent_api_profile_editing_profile_id.clone() {
            if let Some(existing) = AISettings::as_ref(ctx)
                .cli_agent_api_profiles()
                .profiles
                .into_iter()
                .find(|profile| profile.id == profile_id)
            {
                profile.enabled = existing.enabled;
                profile.health = existing.health;
            }
            profile.id = profile_id;
        }
        let make_active = profile.enabled;
        let saved_profile_id = profile.id.clone();
        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.add_cli_agent_api_profile(profile, make_active, ctx);
        });
        self.cli_agent_api_profile_editing_profile_id = Some(saved_profile_id.clone());
        self.cli_agent_api_profile_add_button
            .update(ctx, |button, ctx| {
                button.set_label("保存", ctx);
                button.set_icon(Some(Icon::Check), ctx);
            });
        self.sync_cli_agent_api_profile_mouse_state_handles(ctx);
        self.set_cli_agent_api_profile_save_feedback("供应商已保存", false, ctx);
        self.sync_saved_cli_agent_api_profile_settings(&saved_profile_id, ctx);
        ctx.notify();
    }

    fn set_cli_agent_api_profiles_json_editor_text(
        &mut self,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        self.cli_agent_api_profiles_json_editor
            .update(ctx, |editor, ctx| editor.set_buffer_text(text, ctx));
    }

    fn handle_copy_cli_agent_api_profiles_json(&mut self, ctx: &mut ViewContext<Self>) {
        match AISettings::as_ref(ctx).cli_agent_api_profiles_export_json() {
            Ok(json) => {
                ctx.clipboard()
                    .write(ClipboardContent::plain_text(json.clone()));
                self.set_cli_agent_api_profiles_json_editor_text(&json, ctx);
                ctx.notify();
            }
            Err(error) => {
                log::warn!("{error}");
            }
        }
    }

    fn handle_paste_cli_agent_api_profiles_json(&mut self, ctx: &mut ViewContext<Self>) {
        let clipboard_text = ctx.clipboard().read().plain_text;
        self.set_cli_agent_api_profiles_json_editor_text(&clipboard_text, ctx);
        ctx.notify();
    }

    fn handle_import_cli_agent_api_profiles_json(
        &mut self,
        replace_existing: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let raw_json = self
            .cli_agent_api_profiles_json_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let mut result = Ok(0);
        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            result = if replace_existing {
                settings.replace_cli_agent_api_profiles_json(&raw_json, ctx)
            } else {
                settings.merge_cli_agent_api_profiles_json(&raw_json, ctx)
            };
        });

        match result {
            Ok(profile_count) => {
                log::info!("Imported {profile_count} Agent API profiles");
                if let Ok(json) = AISettings::as_ref(ctx).cli_agent_api_profiles_export_json() {
                    self.set_cli_agent_api_profiles_json_editor_text(&json, ctx);
                }
                self.sync_cli_agent_api_profile_mouse_state_handles(ctx);
                ctx.notify();
            }
            Err(error) => {
                log::warn!("Failed to import Agent API profiles JSON: {error}");
            }
        }
    }

    fn handle_check_cli_agent_api_profile(
        &mut self,
        profile_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(profile) = AISettings::as_ref(ctx)
            .cli_agent_api_profiles()
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
        else {
            return;
        };
        let profile_id = profile.id.clone();
        let checking_health =
            CLIAgentApiProfileHealth::checking(cli_agent_api_health_check_now_ms());
        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.record_cli_agent_api_profile_health(&profile_id, checking_health, ctx);
        });
        ctx.notify();

        ctx.spawn(
            check_cli_agent_api_profile_health(profile),
            move |me, health, ctx| {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.record_cli_agent_api_profile_health(&profile_id, health, ctx);
                });
                me.sync_cli_agent_api_profile_mouse_state_handles(ctx);
                ctx.notify();
            },
        );
    }

    fn handle_fetch_cli_agent_api_profile_models(&mut self, ctx: &mut ViewContext<Self>) {
        let name = self
            .cli_agent_api_profile_name_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let anthropic_base_url = self
            .cli_agent_api_profile_base_url_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let openai_base_url = self
            .cli_agent_api_profile_openai_base_url_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let api_key = self
            .cli_agent_api_profile_api_key_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let model = self
            .cli_agent_api_profile_model_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        let fetch_agent = if openai_base_url.trim().is_empty() {
            self.cli_agent_api_profile_draft_agent
        } else {
            CLIAgent::Codex
        };
        let base_url = if openai_base_url.trim().is_empty() {
            Self::cli_agent_api_profile_base_url_for_agent(
                self.cli_agent_api_profile_draft_agent,
                &anthropic_base_url,
                &openai_base_url,
            )
        } else {
            openai_base_url
        };
        let mut profile = CLIAgentApiProfile::new(
            fetch_agent,
            self.cli_agent_api_profile_draft_environment_id.clone(),
            name,
            base_url,
            api_key,
            model,
        );
        profile.api_format = self
            .cli_agent_api_profile_api_format_editor
            .as_ref(ctx)
            .buffer_text(ctx);
        profile.full_url_mode = matches!(
            self.cli_agent_api_profile_full_url_mode_editor
                .as_ref(ctx)
                .buffer_text(ctx)
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on" | "full"
        );
        self.cli_agent_api_profile_fetching_models = true;
        self.cli_agent_api_profile_fetch_models_request_id = self
            .cli_agent_api_profile_fetch_models_request_id
            .wrapping_add(1);
        let request_id = self.cli_agent_api_profile_fetch_models_request_id;
        self.cli_agent_api_profile_fetch_models_error = None;
        self.cli_agent_api_profile_fetched_models.clear();
        self.cli_agent_api_profile_selected_fetched_model.clear();
        self.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
        self.cli_agent_api_profile_fetch_models_button
            .update(ctx, |button, ctx| {
                button.set_label("获取中", ctx);
                button.set_disabled(true, ctx);
            });
        ctx.notify();

        ctx.spawn(
            fetch_cli_agent_api_models(profile),
            move |me, result, ctx| {
                if me.cli_agent_api_profile_fetch_models_request_id != request_id {
                    return;
                }
                me.cli_agent_api_profile_fetching_models = false;
                me.cli_agent_api_profile_fetch_models_button
                    .update(ctx, |button, ctx| {
                        button.set_label("获取模型", ctx);
                        button.set_disabled(false, ctx);
                    });
                match result {
                    Ok(models) => {
                        me.cli_agent_api_profile_fetched_models = models;
                        me.cli_agent_api_profile_selected_fetched_model = me
                            .cli_agent_api_profile_fetched_models
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        if !me
                            .cli_agent_api_profile_selected_fetched_model
                            .trim()
                            .is_empty()
                            && me
                                .cli_agent_api_profile_model_editor
                                .as_ref(ctx)
                                .buffer_text(ctx)
                                .trim()
                                .is_empty()
                        {
                            let model = me.cli_agent_api_profile_selected_fetched_model.clone();
                            me.cli_agent_api_profile_model_editor
                                .update(ctx, |editor, ctx| editor.set_buffer_text(&model, ctx));
                        }
                        me.cli_agent_api_profile_fetch_models_error = None;
                    }
                    Err(error) => {
                        me.cli_agent_api_profile_fetched_models.clear();
                        me.cli_agent_api_profile_selected_fetched_model.clear();
                        me.cli_agent_api_profile_fetch_models_error = Some(error.clone());
                        log::warn!("Failed to fetch Agent API models: {error}");
                    }
                }
                me.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
                ctx.notify();
            },
        );
    }

    fn cli_agent_api_environment_label(environment_id: &str, app: &AppContext) -> String {
        match environment_id {
            CLI_AGENT_API_ALL_ENVIRONMENTS_ID => "All environments".to_owned(),
            CLI_AGENT_API_LOCAL_ENVIRONMENT_ID => "Local".to_owned(),
            _ => SshRemoteModel::as_ref(app)
                .hosts()
                .iter()
                .find(|host| ssh_remote_environment_id(&host.id) == environment_id)
                .map(|host| format!("SSH {}", host.display_name()))
                .unwrap_or_else(|| environment_id.to_owned()),
        }
    }

    fn masked_api_key(api_key: &str) -> String {
        let trimmed = api_key.trim();
        if trimmed.is_empty() {
            return "No API key".to_owned();
        }

        let suffix = trimmed
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("**** {suffix}")
    }

    fn create_cli_agent_dropdowns(
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<Dropdown<AISettingsPageAction>>> {
        let entries: Vec<(String, CLIAgent)> = AISettings::as_ref(ctx)
            .cli_agent_footer_enabled_commands
            .value()
            .iter()
            .map(|(pattern, agent_value)| {
                (pattern.clone(), CLIAgent::from_serialized_name(agent_value))
            })
            .collect();

        entries
            .into_iter()
            .map(|(pattern_clone, current_agent)| {
                ctx.add_typed_action_view(move |ctx| {
                    let mut dropdown = Dropdown::new(ctx);
                    dropdown.set_top_bar_max_width(160.);
                    dropdown.set_menu_width(180., ctx);
                    dropdown.set_main_axis_size(MainAxisSize::Min, ctx);

                    let mut items: Vec<MenuItem<DropdownAction>> = Vec::new();

                    for agent in all::<CLIAgent>() {
                        if matches!(agent, CLIAgent::Unknown) {
                            continue;
                        }
                        let icon = agent.icon();
                        let mut fields = MenuItemFields::new(agent.display_name())
                            .with_on_select_action(DropdownAction::select_action_and_close(
                                AISettingsPageAction::SetCLIAgentForCommand {
                                    pattern: pattern_clone.clone(),
                                    agent: Some(agent),
                                },
                            ));
                        if let Some(icon) = icon {
                            fields = fields.with_icon(icon);
                        }
                        items.push(fields.into_item());
                    }

                    items.push(
                        MenuItemFields::new("Other")
                            .with_on_select_action(DropdownAction::select_action_and_close(
                                AISettingsPageAction::SetCLIAgentForCommand {
                                    pattern: pattern_clone.clone(),
                                    agent: None,
                                },
                            ))
                            .into_item(),
                    );

                    dropdown.set_rich_items(items, ctx);

                    dropdown.set_menu_header_text_override(|label| {
                        if label == "Other" {
                            "Select coding agent".to_string()
                        } else {
                            label.to_string()
                        }
                    });

                    let selected_name = if matches!(current_agent, CLIAgent::Unknown) {
                        "Other"
                    } else {
                        current_agent.display_name()
                    };
                    dropdown.set_selected_by_name(selected_name, ctx);

                    dropdown
                })
            })
            .collect()
    }
}

fn cli_agent_api_health_check_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn cli_agent_api_health_check_url(profile: &CLIAgentApiProfile) -> Result<String, String> {
    let base_url = profile.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("No base URL configured".to_owned());
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("Base URL must start with http:// or https://".to_owned());
    }
    if base_url.ends_with("/models") {
        return Ok(base_url.to_owned());
    }

    let version_path = if profile.agent() == CLIAgent::Gemini {
        "v1beta"
    } else {
        "v1"
    };
    if base_url.ends_with("/v1") || base_url.ends_with("/v1beta") {
        Ok(format!("{base_url}/models"))
    } else {
        Ok(format!("{base_url}/{version_path}/models"))
    }
}

async fn check_cli_agent_api_profile_health(
    profile: CLIAgentApiProfile,
) -> CLIAgentApiProfileHealth {
    let checked_at_epoch_ms = cli_agent_api_health_check_now_ms();
    let Ok(url) = cli_agent_api_health_check_url(&profile) else {
        return CLIAgentApiProfileHealth::failed(
            checked_at_epoch_ms,
            0,
            0,
            cli_agent_api_health_check_url(&profile)
                .err()
                .unwrap_or_else(|| "Invalid health check URL".to_owned()),
        );
    };

    let start = Instant::now();
    let client = http_client::Client::new();
    let mut request = client.get(url).timeout(Duration::from_secs(10));
    let api_key = profile.api_key.trim();
    if !api_key.is_empty() {
        match profile.agent() {
            CLIAgent::Claude => {
                request = request
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01");
            }
            CLIAgent::Gemini => {
                request = request.header("x-goog-api-key", api_key);
            }
            _ => {
                request = request.bearer_auth(api_key);
            }
        }
    }

    match request.send().await {
        Ok(response) => {
            let latency_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let status = response.status();
            if status.is_success() {
                CLIAgentApiProfileHealth::healthy(checked_at_epoch_ms, latency_ms, status.as_u16())
            } else {
                CLIAgentApiProfileHealth::failed(
                    checked_at_epoch_ms,
                    latency_ms,
                    status.as_u16(),
                    format!("HTTP {status}"),
                )
            }
        }
        Err(error) => {
            let latency_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
            CLIAgentApiProfileHealth::failed(checked_at_epoch_ms, latency_ms, 0, error.to_string())
        }
    }
}

async fn fetch_cli_agent_api_models(profile: CLIAgentApiProfile) -> Result<Vec<String>, String> {
    let url = cli_agent_api_health_check_url(&profile)?;
    let client = http_client::Client::new();
    let mut request = client.get(url).timeout(Duration::from_secs(15));
    let api_key = profile.api_key.trim();
    if !api_key.is_empty() {
        match profile.agent() {
            CLIAgent::Claude => {
                request = request
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01");
            }
            CLIAgent::Gemini => {
                request = request.header("x-goog-api-key", api_key);
            }
            _ => {
                request = request.bearer_auth(api_key);
            }
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read response body: {error}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {body}"));
    }

    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|error| format!("invalid models JSON: {error}"))?;
    let mut models = Vec::new();
    if let Some(data) = value.get("data").and_then(serde_json::Value::as_array) {
        for item in data {
            if let Some(id) = item.get("id").and_then(serde_json::Value::as_str) {
                models.push(id.to_owned());
            }
        }
    }
    if let Some(data) = value.get("models").and_then(serde_json::Value::as_array) {
        for item in data {
            if let Some(name) = item.get("name").and_then(serde_json::Value::as_str) {
                models.push(name.trim_start_matches("models/").to_owned());
            } else if let Some(name) = item.get("displayName").and_then(serde_json::Value::as_str) {
                models.push(name.to_owned());
            }
        }
    }
    models.retain(|model| !model.trim().is_empty());
    models.sort();
    models.dedup();

    if models.is_empty() {
        Err("provider returned no models".to_owned())
    } else {
        Ok(models)
    }
}

impl View for AISettingsPageView {
    fn ui_name() -> &'static str {
        "AISettingsPage"
    }

    fn render(&self, app: &warpui::AppContext) -> Box<dyn warpui::Element> {
        self.page.render(self, app)
    }
}

pub enum AISettingsPageEvent {
    FocusModal,
    OpenAIFactCollection,
    OpenMCPServerCollection,
    OpenExecutionProfileEditor(ClientProfileId),
    SignupAnonymousUser,
    ShowModal,
    HideModal,
}

impl Entity for AISettingsPageView {
    type Event = AISettingsPageEvent;
}

#[derive(Debug, Clone, PartialEq)]
pub enum AISettingsPageAction {
    OpenUrl(String),
    SetVoiceInputToggleKey(VoiceInputToggleKey),
    ToggleGlobalAI,
    ToggleActiveAI,
    ToggleIntelligentAutosuggestions,
    TogglePromptSuggestions,
    ToggleCodeSuggestions,
    ToggleNaturalLanguageAutosuggestions,
    ToggleSharedTitleGeneration,
    ToggleGitOperationsAutogen,
    ToggleAIInputAutoDetection,
    ToggleNLDInTerminal,
    ToggleCLIAgentToolbar,
    ToggleUseAgentToolbar,
    ToggleVoiceInput,
    ToggleCanUseWarpCreditsForFallback,
    HyperlinkClick(HyperlinkUrl),
    ToggleCodebaseContext,
    ToggleShowInputHintText,
    ToggleShowAgentTips,
    ToggleShowOzUpdatesInZeroState,
    SetThinkingDisplayMode(ThinkingDisplayMode),
    SetPromptSubmissionMode(PromptSubmissionMode),
    AttemptLoginGatedUpgrade,
    RemoveCLIAgentToolbarEnabledCommand(String),
    RemoveFromCommandExecutionAllowlist(AgentModeCommandExecutionPredicate),
    RemoveFromCommandExecutionDenylist(AgentModeCommandExecutionPredicate),
    OpenAIFactCollection,
    OpenMCPServerCollection,
    OpenExecutionProfileEditor(ClientProfileId),
    SetBaseModel(LLMId),
    SetCodingModel(LLMId),
    /// Called while the user is actively dragging the context window slider.
    ContextWindowSliderDragged(u32),
    /// Called when the user commits a new context window value (slider drop or
    /// input box commit).
    SetContextWindowSize(u32),
    SetAutonomyReadonlyCommandsSetting,
    SetAutonomySupervisedSetting,
    SetCodingPermission(AgentModeCodingPermissionsType),
    RemoveDirectoryFromCodeReadAllowlist(PathBuf),
    ToggleRules,
    ToggleRuleSuggestions,
    ToggleWarpDriveContext,
    SetApplyCodeDiffs(ActionPermission),
    SetReadFiles(ActionPermission),
    SetExecuteCommands(ActionPermission),
    SetWriteToPty(WriteToPtyPermission),
    SetMCPPermissions(ActionPermission),
    RemoveFromProfileDirectoryAllowlist(PathBuf),
    RemoveFromProfileCommandDenylist(AgentModeCommandExecutionPredicate),
    RemoveFromProfileCommandAllowlist(AgentModeCommandExecutionPredicate),
    ToggleShowBaseModelPickerInPrompt,
    AddToMCPAllowlist(uuid::Uuid),
    RemoveFromMCPAllowlist(uuid::Uuid),
    AddToMCPDenylist(uuid::Uuid),
    RemoveFromMCPDenylist(uuid::Uuid),
    CreateProfile,
    SignupAnonymousUser,
    ToggleAwsBedrockAutoLogin,
    ToggleAwsBedrockCredentialsEnabled,
    RefreshAwsBedrockCredentials,
    ToggleCloudAgentComputerUse,
    ToggleFileBasedMcp,
    ToggleIncludeAgentCommandsInHistory,
    ToggleAgentAttribution,

    // Custom inference
    OpenAddCustomEndpointModal,
    OpenEditCustomEndpointModal(usize),

    #[cfg(feature = "local_fs")]
    SetConversationLayout(crate::util::file::external_editor::settings::OpenConversationPreference),
    ToggleCloudHandoff,
    ToggleAmpersandHandoff,
    ToggleAutoHandoffOnSleep,
    ToggleShowConversationHistory,
    ToggleAutoToggleRichInput,
    ToggleAutoOpenRichInputOnCLIAgentStart,
    ToggleAutoDismissRichInputAfterSubmit,
    SetCLIAgentBuiltinPromptMode {
        agent: CLIAgent,
        mode: CLIAgentBuiltinPromptMode,
    },
    ToggleCLIAgentApiTakeover,
    SetCLIAgentApiProfilePreset(String),
    SetCLIAgentApiProfileDraftAgent(CLIAgent),
    SetCLIAgentApiProfileDraftEnvironment(String),
    OpenAddCLIAgentApiProfileModal,
    AddCLIAgentApiProfile,
    FetchCLIAgentApiProfileModels,
    SelectCLIAgentApiProfileFetchedModel(String),
    ToggleCLIAgentApiProfileAddModel,
    ToggleCLIAgentApiProfileModelMapping,
    AddCLIAgentApiProfileDraftModel,
    AddCLIAgentApiProfileSelectedFetchedModel,
    UseCLIAgentApiProfileModel(String),
    RemoveCLIAgentApiProfileModel(String),
    EditCLIAgentApiProfile(String),
    CancelEditCLIAgentApiProfile,
    RemoveCLIAgentApiProfile(String),
    CopyCLIAgentApiProfilesJson,
    PasteCLIAgentApiProfilesJson,
    MergeCLIAgentApiProfilesJson,
    ReplaceCLIAgentApiProfilesJson,
    CheckCLIAgentApiProfile(String),
    SetCLIAgentApiProfileEnabled {
        profile_id: String,
        enabled: bool,
    },
    SetActiveCLIAgentApiProfile {
        agent: CLIAgent,
        environment_id: String,
        profile_id: String,
    },
    SaveAgentSessionId,
    EditAgentSessionId(String),
    CancelEditAgentSessionId,
    RemoveAgentSessionId(String),
    SetCLIAgentForCommand {
        pattern: String,
        agent: Option<CLIAgent>,
    },
}

impl From<&AISettingsPageAction> for LoginGatedFeature {
    fn from(val: &AISettingsPageAction) -> LoginGatedFeature {
        use AISettingsPageAction::*;
        match val {
            AttemptLoginGatedUpgrade => "Upgrade AI Usage",
            _ => "Unknown reason",
        }
    }
}

impl AISettingsPageView {
    fn parse_agent_session_id_agent(agent: &str) -> Option<CLIAgent> {
        match agent.trim().to_lowercase().as_str() {
            "codex" => Some(CLIAgent::Codex),
            "claude" | "claude code" => Some(CLIAgent::Claude),
            "opencode" | "open code" => Some(CLIAgent::OpenCode),
            _ => None,
        }
    }

    fn agent_session_id_editor_text(editor: &ViewHandle<EditorView>, ctx: &AppContext) -> String {
        editor.as_ref(ctx).buffer_text(ctx).trim().to_owned()
    }

    fn agent_session_id_project_path(text: &str) -> PathBuf {
        let text = text.trim();
        if text == "~" {
            return std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(text));
        }
        if let Some(rest) = text.strip_prefix("~/") {
            return std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(rest))
                .unwrap_or_else(|| PathBuf::from(text));
        }
        PathBuf::from(text)
    }

    fn set_agent_session_id_editor_text(
        editor: &ViewHandle<EditorView>,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        editor.update(ctx, |editor, ctx| editor.set_buffer_text(text, ctx));
    }

    fn clear_agent_session_id_editors(&mut self, ctx: &mut ViewContext<Self>) {
        self.agent_session_id_editing_record_id = None;
        Self::set_agent_session_id_editor_text(&self.agent_session_id_agent_editor, "codex", ctx);
        Self::set_agent_session_id_editor_text(&self.agent_session_id_project_editor, "", ctx);
        Self::set_agent_session_id_editor_text(&self.agent_session_id_value_editor, "", ctx);
        Self::set_agent_session_id_editor_text(&self.agent_session_id_title_editor, "", ctx);
    }
}

impl TypedActionView for AISettingsPageView {
    type Action = AISettingsPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AISettingsPageAction::OpenUrl(url) => {
                ctx.open_url(url.as_str());
            }
            AISettingsPageAction::SetVoiceInputToggleKey(key) => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.voice_input_toggle_key.set_value(*key, ctx));
                    report_if_error!(settings
                        .explicitly_interacted_with_voice
                        .set_value(true, ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleGlobalAI => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.is_any_ai_enabled.toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleGlobalAI {
                                is_ai_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Global AI setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleActiveAI => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .is_active_ai_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleActiveAI {
                                is_active_ai_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Active AI setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleIntelligentAutosuggestions => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .intelligent_autosuggestions_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleIntelligentAutosuggestionsSetting {
                                is_intelligent_autosuggestions_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Next Command setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::TogglePromptSuggestions => {
                if !UserWorkspaces::as_ref(ctx).is_prompt_suggestions_toggleable() {
                    return;
                }
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .prompt_suggestions_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::TogglePromptSuggestionsSetting {
                                is_prompt_suggestions_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Prompt Suggestions setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleCodeSuggestions => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .code_suggestions_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleCodeSuggestionsSetting {
                                source: ToggleCodeSuggestionsSettingSource::Settings,
                                is_code_suggestions_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Code Suggestions setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleNaturalLanguageAutosuggestions => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .natural_language_autosuggestions_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleNaturalLanguageAutosuggestionsSetting {
                                is_natural_language_autosuggestions_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to set value for Natural Language Autosuggestions setting: {e:?}"
                        );
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleSharedTitleGeneration => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .shared_block_title_generation_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(_new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleSharedBlockTitleGenerationSetting {
                                is_shared_block_title_generation_enabled: true,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to set value for Shared Block Title Generation setting: {e:?}"
                        );
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleGitOperationsAutogen => {
                if !UserWorkspaces::as_ref(ctx).is_git_operations_ai_enabled() {
                    return;
                }
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .git_operations_autogen_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleGitOperationsAutogenSetting {
                                is_git_operations_autogen_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Git Operations Autogen setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleAIInputAutoDetection => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .ai_autodetection_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::AgentModeToggleAutoDetectionSetting {
                                is_autodetection_enabled: new_value,
                                origin: AgentModeAutoDetectionSettingOrigin::SettingsPage
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Input Auto-detection: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleNLDInTerminal => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .nld_in_terminal_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(_new_value) => {}
                    Err(e) => {
                        log::warn!("Failed to set value for NLD in Terminal: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleCLIAgentToolbar => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .should_render_cli_agent_footer
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleCLIAgentToolbarSetting {
                                is_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for CLI Agent Footer setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleAutoToggleRichInput => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.auto_toggle_rich_input.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAutoOpenRichInputOnCLIAgentStart => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .auto_open_rich_input_on_cli_agent_start
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAutoDismissRichInputAfterSubmit => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .auto_dismiss_rich_input_after_submit
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleUseAgentToolbar => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .should_render_use_agent_footer_for_user_commands
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleUseAgentToolbarSetting {
                                is_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Use Agent Footer setting: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleCodebaseContext => {
                match CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.codebase_context_enabled.toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleCodebaseContext {
                                is_codebase_context_enabled: new_value
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Codebase Context: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleVoiceInput => {
                match AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings
                        .voice_input_enabled_internal
                        .toggle_and_save_value(ctx)
                }) {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleVoiceInputSetting {
                                is_voice_input_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Voice Input: {e:?}");
                    }
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleCanUseWarpCreditsForFallback => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .can_use_warp_credits_for_fallback
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::HyperlinkClick(hyperlink) => {
                ctx.notify();
                ctx.open_url(&hyperlink.url);
            }
            AISettingsPageAction::ToggleShowInputHintText => {
                InputSettings::handle(ctx).update(ctx, |input_settings, ctx| {
                    report_if_error!(input_settings.show_hint_text.toggle_and_save_value(ctx));
                    send_telemetry_from_ctx!(
                        // We purposely keep the FeaturesPageAction event, even though we have moved the setting to AI settings.
                        TelemetryEvent::FeaturesPageAction {
                            action: "ToggleShowInputHintText".to_string(),
                            value: format!("{}", *input_settings.show_hint_text),
                        },
                        ctx
                    );
                });
            }
            AISettingsPageAction::ToggleShowAgentTips => {
                InputSettings::handle(ctx).update(ctx, |input_settings, ctx| match input_settings
                    .show_agent_tips
                    .toggle_and_save_value(ctx)
                {
                    Ok(new_value) => {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::ToggleShowAgentTips {
                                is_enabled: new_value,
                            },
                            ctx
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to set value for Show Agent Tips setting: {e:?}");
                    }
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleShowOzUpdatesInZeroState => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .should_show_oz_updates_in_zero_state
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::SetThinkingDisplayMode(mode) => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.thinking_display_mode.set_value(*mode, ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::SetPromptSubmissionMode(mode) => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .default_prompt_submission_mode
                        .set_value(*mode, ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::AttemptLoginGatedUpgrade => {
                AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                    auth_manager.attempt_login_gated_feature(
                        action.into(),
                        AuthViewVariant::RequireLoginCloseable,
                        ctx,
                    )
                });
            }
            AISettingsPageAction::RemoveCLIAgentToolbarEnabledCommand(command) => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.remove_cli_agent_footer_enabled_command(command, ctx);
                });
            }
            AISettingsPageAction::SetCLIAgentBuiltinPromptMode { agent, mode } => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.set_cli_agent_builtin_prompt_mode(*agent, *mode, ctx);
                });
            }
            AISettingsPageAction::ToggleCLIAgentApiTakeover => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .cli_agent_api_takeover_enabled
                        .toggle_and_save_value(ctx));
                });
                if !AISettings::as_ref(ctx).is_cli_agent_api_takeover_enabled() {
                    AgentRuntimeSettingsModel::handle(ctx).update(ctx, |settings, ctx| {
                        for agent in AISettings::cli_agent_api_profile_agents() {
                            settings.set_model(agent, DEFAULT_CLI_AGENT_MODEL_LABEL, ctx);
                        }
                    });
                }
            }
            AISettingsPageAction::SetCLIAgentApiProfilePreset(preset_id) => {
                self.handle_set_cli_agent_api_profile_preset(preset_id, ctx);
            }
            AISettingsPageAction::SetCLIAgentApiProfileDraftAgent(agent) => {
                self.set_cli_agent_api_profile_preset_selection(
                    CLI_AGENT_API_CUSTOM_PRESET_ID,
                    ctx,
                );
                self.cli_agent_api_profile_draft_agent = *agent;
                self.cli_agent_api_profile_fetched_models.clear();
                self.cli_agent_api_profile_selected_fetched_model.clear();
                self.cli_agent_api_profile_fetch_models_error = None;
                self.cli_agent_api_profile_fetching_models = false;
                self.cli_agent_api_profile_fetch_models_request_id = self
                    .cli_agent_api_profile_fetch_models_request_id
                    .wrapping_add(1);
                self.refresh_cli_agent_api_profile_model_picker_dropdown(ctx);
                self.cli_agent_api_profile_fetch_models_button
                    .update(ctx, |button, ctx| {
                        button.set_label("获取模型", ctx);
                        button.set_disabled(false, ctx);
                    });
                self.cli_agent_api_profile_api_format_editor
                    .update(ctx, |editor, ctx| {
                        editor.set_buffer_text(Self::cli_agent_api_default_api_format(*agent), ctx);
                    });
                self.cli_agent_api_profile_auth_env_var_editor
                    .update(ctx, |editor, ctx| {
                        editor
                            .set_buffer_text(Self::cli_agent_api_default_auth_env_var(*agent), ctx);
                    });
                ctx.notify();
            }
            AISettingsPageAction::SetCLIAgentApiProfileDraftEnvironment(environment_id) => {
                self.cli_agent_api_profile_draft_environment_id = environment_id.clone();
                ctx.notify();
            }
            AISettingsPageAction::OpenAddCLIAgentApiProfileModal => {
                self.reset_cli_agent_api_profile_form(ctx);
                self.cli_agent_api_profile_editor_open = false;
                ctx.emit(AISettingsPageEvent::HideModal);
                ctx.notify();
            }
            AISettingsPageAction::AddCLIAgentApiProfile => {
                self.handle_add_cli_agent_api_profile(ctx);
            }
            AISettingsPageAction::FetchCLIAgentApiProfileModels => {
                self.handle_fetch_cli_agent_api_profile_models(ctx);
            }
            AISettingsPageAction::SelectCLIAgentApiProfileFetchedModel(model) => {
                self.cli_agent_api_profile_selected_fetched_model = model.clone();
                if !model.trim().is_empty() {
                    self.cli_agent_api_profile_model_editor
                        .update(ctx, |editor, ctx| editor.set_buffer_text(model, ctx));
                }
                ctx.notify();
            }
            AISettingsPageAction::ToggleCLIAgentApiProfileAddModel => {
                self.cli_agent_api_profile_add_model_open =
                    !self.cli_agent_api_profile_add_model_open;
                self.sync_cli_agent_api_profile_add_model_button(ctx);
                ctx.notify();
            }
            AISettingsPageAction::ToggleCLIAgentApiProfileModelMapping => {
                self.cli_agent_api_profile_model_mapping_open =
                    !self.cli_agent_api_profile_model_mapping_open;
                ctx.notify();
            }
            AISettingsPageAction::AddCLIAgentApiProfileDraftModel => {
                let model = self
                    .cli_agent_api_profile_model_editor
                    .as_ref(ctx)
                    .buffer_text(ctx);
                self.handle_add_cli_agent_api_profile_model(&model, true, ctx);
            }
            AISettingsPageAction::AddCLIAgentApiProfileSelectedFetchedModel => {
                let model = self.cli_agent_api_profile_selected_fetched_model.clone();
                self.handle_add_cli_agent_api_profile_model(&model, false, ctx);
            }
            AISettingsPageAction::UseCLIAgentApiProfileModel(model) => {
                self.handle_add_cli_agent_api_profile_model(model, true, ctx);
            }
            AISettingsPageAction::RemoveCLIAgentApiProfileModel(model) => {
                self.handle_remove_cli_agent_api_profile_model(model, ctx);
            }
            AISettingsPageAction::EditCLIAgentApiProfile(profile_id) => {
                self.handle_edit_cli_agent_api_profile(profile_id, ctx);
            }
            AISettingsPageAction::CancelEditCLIAgentApiProfile => {
                self.reset_cli_agent_api_profile_form(ctx);
                self.cli_agent_api_profile_editor_open = false;
                ctx.emit(AISettingsPageEvent::HideModal);
                ctx.notify();
            }
            AISettingsPageAction::RemoveCLIAgentApiProfile(profile_id) => {
                let was_editing = self.cli_agent_api_profile_editing_profile_id.as_deref()
                    == Some(profile_id.as_str());
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.remove_cli_agent_api_profile(profile_id, ctx);
                });
                if was_editing {
                    self.reset_cli_agent_api_profile_form(ctx);
                    self.cli_agent_api_profile_editor_open = false;
                    ctx.emit(AISettingsPageEvent::HideModal);
                }
                self.sync_cli_agent_api_profile_mouse_state_handles(ctx);
                ctx.notify();
            }
            AISettingsPageAction::CopyCLIAgentApiProfilesJson => {
                self.handle_copy_cli_agent_api_profiles_json(ctx);
            }
            AISettingsPageAction::PasteCLIAgentApiProfilesJson => {
                self.handle_paste_cli_agent_api_profiles_json(ctx);
            }
            AISettingsPageAction::MergeCLIAgentApiProfilesJson => {
                self.handle_import_cli_agent_api_profiles_json(false, ctx);
            }
            AISettingsPageAction::ReplaceCLIAgentApiProfilesJson => {
                self.handle_import_cli_agent_api_profiles_json(true, ctx);
            }
            AISettingsPageAction::CheckCLIAgentApiProfile(profile_id) => {
                self.handle_check_cli_agent_api_profile(profile_id, ctx);
            }
            AISettingsPageAction::SetCLIAgentApiProfileEnabled {
                profile_id,
                enabled,
            } => {
                let profile_before_change = AISettings::as_ref(ctx)
                    .cli_agent_api_profiles()
                    .profiles
                    .into_iter()
                    .find(|profile| profile.id == *profile_id);
                let profile_to_activate = if *enabled {
                    profile_before_change.clone()
                } else {
                    None
                };
                let sync_enabled_profile_id = profile_to_activate
                    .as_ref()
                    .filter(|profile| profile.agent() == CLIAgent::Claude)
                    .map(|profile| profile.id.clone());
                let sync_after_disable = if !*enabled {
                    profile_before_change
                        .as_ref()
                        .filter(|profile| profile.agent() == CLIAgent::Claude)
                        .map(|profile| (profile.agent(), profile.environment_id.clone()))
                } else {
                    None
                };
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.set_cli_agent_api_profile_enabled(profile_id, *enabled, ctx);
                    if let Some(profile) = profile_to_activate.as_ref() {
                        settings.set_active_cli_agent_api_profile(
                            profile.agent(),
                            &profile.environment_id,
                            &profile.id,
                            ctx,
                        );
                    }
                });
                if let Some(profile_id) = sync_enabled_profile_id {
                    self.sync_cli_agent_api_profile_settings(&profile_id, "供应商已启用", ctx);
                } else if let Some((agent, environment_id)) = sync_after_disable {
                    self.sync_active_cli_agent_api_profile_settings(
                        agent,
                        &environment_id,
                        "供应商已禁用",
                        ctx,
                    );
                }
                ctx.notify();
            }
            AISettingsPageAction::SetActiveCLIAgentApiProfile {
                agent,
                environment_id,
                profile_id,
            } => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.set_active_cli_agent_api_profile(
                        *agent,
                        environment_id,
                        profile_id,
                        ctx,
                    );
                });
                self.sync_active_cli_agent_api_profile_settings(
                    *agent,
                    environment_id,
                    "供应商已切换",
                    ctx,
                );
            }
            AISettingsPageAction::SetCLIAgentForCommand { pattern, agent } => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.set_cli_agent_for_command(pattern, *agent, ctx);
                });
            }
            AISettingsPageAction::SaveAgentSessionId => {
                let agent_text =
                    Self::agent_session_id_editor_text(&self.agent_session_id_agent_editor, ctx);
                let Some(agent) = Self::parse_agent_session_id_agent(&agent_text) else {
                    log::warn!("Invalid agent for session id record: {agent_text}");
                    return;
                };
                let project_path = Self::agent_session_id_project_path(
                    &Self::agent_session_id_editor_text(
                        &self.agent_session_id_project_editor,
                        ctx,
                    ),
                );
                let agent_session_id =
                    Self::agent_session_id_editor_text(&self.agent_session_id_value_editor, ctx);
                let title =
                    Self::agent_session_id_editor_text(&self.agent_session_id_title_editor, ctx);
                let record_id = self.agent_session_id_editing_record_id.clone();
                let result = AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.save_session_id_record(
                        record_id.as_deref(),
                        agent,
                        project_path,
                        agent_session_id,
                        title,
                        ctx,
                    )
                });
                if let Err(error) = result {
                    log::warn!("Failed to save agent session id record: {error}");
                    return;
                }
                self.clear_agent_session_id_editors(ctx);
                ctx.notify();
            }
            AISettingsPageAction::EditAgentSessionId(session_id) => {
                let record = AgentSessionsModel::as_ref(ctx).session(session_id).cloned();
                let Some(record) = record else {
                    return;
                };
                self.agent_session_id_editing_record_id = Some(record.id.clone());
                Self::set_agent_session_id_editor_text(
                    &self.agent_session_id_agent_editor,
                    record.agent.command_prefix(),
                    ctx,
                );
                Self::set_agent_session_id_editor_text(
                    &self.agent_session_id_project_editor,
                    &record.project_path.to_string_lossy(),
                    ctx,
                );
                Self::set_agent_session_id_editor_text(
                    &self.agent_session_id_value_editor,
                    record.agent_session_id.as_deref().unwrap_or_default(),
                    ctx,
                );
                Self::set_agent_session_id_editor_text(
                    &self.agent_session_id_title_editor,
                    &record.title,
                    ctx,
                );
                ctx.notify();
            }
            AISettingsPageAction::CancelEditAgentSessionId => {
                self.clear_agent_session_id_editors(ctx);
                ctx.notify();
            }
            AISettingsPageAction::RemoveAgentSessionId(session_id) => {
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.delete_session(session_id, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromCommandExecutionAllowlist(cmd) => {
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(model.remove_command_from_autoexecution_allowlist(cmd, ctx));
                })
            }
            AISettingsPageAction::RemoveFromCommandExecutionDenylist(cmd) => {
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(model.remove_command_from_denylist(cmd, ctx));
                })
            }
            AISettingsPageAction::OpenAIFactCollection => {
                ctx.emit(AISettingsPageEvent::OpenAIFactCollection)
            }
            AISettingsPageAction::OpenMCPServerCollection => {
                ctx.emit(AISettingsPageEvent::OpenMCPServerCollection)
            }
            AISettingsPageAction::OpenExecutionProfileEditor(profile_id) => {
                ctx.emit(AISettingsPageEvent::OpenExecutionProfileEditor(*profile_id))
            }
            AISettingsPageAction::SetBaseModel(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles_model, ctx| {
                    let profile_id = *profiles_model.active_profile(None, ctx).id();
                    profiles_model.set_base_model(profile_id, Some(id.clone()), ctx);
                    profiles_model.set_context_window_limit(profile_id, None, ctx);
                });
                self.sync_context_window_editor(ctx, true);
                ctx.notify();
            }
            AISettingsPageAction::SetCodingModel(id) => {
                LLMPreferences::handle(ctx).update(ctx, |prefs, ctx| {
                    prefs.update_preferred_coding_llm(id, None, ctx);
                });
            }
            AISettingsPageAction::ContextWindowSliderDragged(value) => {
                if !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                    self.sync_context_window_editor(ctx, true);
                    return;
                }
                if Self::configurable_context_window(ctx).is_some() {
                    self.dragged_context_window_value = Some(*value);
                    let formatted = value.to_string();
                    self.context_window_editor.update(ctx, |editor, ctx| {
                        editor.system_reset_buffer_text(&formatted, ctx);
                    });
                    ctx.notify();
                }
            }
            AISettingsPageAction::SetContextWindowSize(value) => {
                self.dragged_context_window_value = None;
                if !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
                    self.sync_context_window_editor(ctx, true);
                    return;
                }
                let Some(cw) = Self::configurable_context_window(ctx) else {
                    return;
                };
                let clamped = (*value).clamp(cw.min, cw.max);
                AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles_model, ctx| {
                    let profile_id = *profiles_model.active_profile(None, ctx).id();
                    profiles_model.set_context_window_limit(profile_id, Some(clamped), ctx);
                });
                self.sync_context_window_editor(ctx, true);
                ctx.notify();
            }
            AISettingsPageAction::SetAutonomyReadonlyCommandsSetting
            | AISettingsPageAction::SetAutonomySupervisedSetting => {
                let readonly_cmd_execution_enabled = matches!(
                    action,
                    AISettingsPageAction::SetAutonomyReadonlyCommandsSetting
                );
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    match model.set_should_autoexecute_readonly_commands(
                        readonly_cmd_execution_enabled,
                        ctx,
                    ) {
                        Ok(_) => {
                            send_telemetry_from_ctx!(
                                TelemetryEvent::ToggledAgentModeAutoexecuteReadonlyCommandsSetting {
                                    src: AutonomySettingToggleSource::SettingsPage,
                                    enabled: readonly_cmd_execution_enabled,
                                },
                                ctx);
                        }
                        Err(e) => report_error!(e),
                    }
                });
            }
            AISettingsPageAction::SetCodingPermission(p) => {
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    match model.set_coding_permissions(*p, ctx) {
                        Ok(_) => {
                            send_telemetry_from_ctx!(
                                TelemetryEvent::ChangedAgentModeCodingPermissions {
                                    src: AutonomySettingToggleSource::SettingsPage,
                                    new: *p,
                                },
                                ctx
                            );
                        }
                        Err(e) => report_error!(e),
                    }
                });
            }
            AISettingsPageAction::SetApplyCodeDiffs(permission) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    model.set_apply_code_diffs(*profile.id(), permission, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::SetReadFiles(permission) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    model.set_read_files(*profile.id(), permission, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::SetExecuteCommands(permission) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    model.set_execute_commands(*profile.id(), permission, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::SetWriteToPty(permission) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    model.set_write_to_pty(*profile.id(), permission, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::SetMCPPermissions(permission) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    model.set_mcp_permissions(*profile.id(), permission, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveDirectoryFromCodeReadAllowlist(dir) => {
                BlocklistAIPermissions::handle(ctx).update(ctx, |model, ctx| {
                    report_if_error!(
                        model.remove_filepath_from_code_read_allowlist(dir.to_owned(), ctx)
                    );
                });
            }
            AISettingsPageAction::ToggleRules => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings.memory_enabled.toggle_and_save_value(ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleRuleSuggestions => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings
                        .rule_suggestions_enabled_internal
                        .toggle_and_save_value(ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleWarpDriveContext => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings
                        .warp_drive_context_enabled
                        .toggle_and_save_value(ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromProfileDirectoryAllowlist(path_buf) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.remove_from_directory_allowlist(
                        *profile_id,
                        &PathBuf::from(path_buf),
                        ctx,
                    );
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromProfileCommandDenylist(cmd) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();

                    model.remove_from_command_denylist(*profile_id, cmd, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromProfileCommandAllowlist(command) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();

                    model.remove_from_command_allowlist(*profile_id, command, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleShowBaseModelPickerInPrompt => {
                SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if let Err(e) = settings
                        .show_model_selectors_in_prompt
                        .toggle_and_save_value(ctx)
                    {
                        log::warn!(
                            "Failed to set value for Show Base Model Picker in Prompt: {e:?}"
                        );
                    }
                });
                ctx.notify();
            }
            AISettingsPageAction::AddToMCPAllowlist(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.add_to_mcp_allowlist(*profile_id, id, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromMCPAllowlist(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.remove_from_mcp_allowlist(*profile_id, id, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::AddToMCPDenylist(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.add_to_mcp_denylist(*profile_id, id, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::RemoveFromMCPDenylist(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
                    let profile = model.default_profile(ctx);
                    let profile_id = profile.id();
                    model.remove_from_mcp_denylist(*profile_id, id, ctx);
                });
                ctx.notify();
            }
            AISettingsPageAction::CreateProfile => {
                let new_profile_id = AIExecutionProfilesModel::handle(ctx)
                    .update(ctx, |model, ctx| model.create_profile(ctx));

                if let Some(profile_id) = new_profile_id {
                    self.profile_views = Self::create_profile_views(ctx);
                    ctx.emit(AISettingsPageEvent::OpenExecutionProfileEditor(profile_id));
                }
                ctx.notify();
            }
            AISettingsPageAction::SignupAnonymousUser => {
                ctx.emit(AISettingsPageEvent::SignupAnonymousUser);
            }
            AISettingsPageAction::ToggleAwsBedrockAutoLogin => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.aws_bedrock_auto_login.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAwsBedrockCredentialsEnabled => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .aws_bedrock_credentials_enabled
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::RefreshAwsBedrockCredentials => {
                #[cfg(not(target_family = "wasm"))]
                ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    drop(refresh_aws_credentials(manager, ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleCloudAgentComputerUse => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .cloud_agent_computer_use_enabled
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleFileBasedMcp => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.file_based_mcp_enabled.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleIncludeAgentCommandsInHistory => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .include_agent_commands_in_history
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            #[cfg(feature = "local_fs")]
            AISettingsPageAction::SetConversationLayout(layout) => {
                crate::util::file::external_editor::EditorSettings::handle(ctx).update(
                    ctx,
                    |settings, ctx| {
                        report_if_error!(settings
                            .open_conversation_layout_preference
                            .set_value(*layout, ctx));
                    },
                );
                send_telemetry_from_ctx!(
                    TelemetryEvent::FeaturesPageAction {
                        action: "SetConversationLayout".to_string(),
                        value: format!("{layout:?}")
                    },
                    ctx
                );
                ctx.notify();
            }
            AISettingsPageAction::ToggleShowConversationHistory => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .show_conversation_history
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::OpenAddCustomEndpointModal => {
                self.show_add_custom_endpoint_modal(ctx);
            }
            AISettingsPageAction::OpenEditCustomEndpointModal(index) => {
                self.show_edit_custom_endpoint_modal(*index, ctx);
            }
            AISettingsPageAction::ToggleCloudHandoff => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .should_force_disable_cloud_handoff
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAmpersandHandoff => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .should_force_disable_ampersand_handoff
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAutoHandoffOnSleep => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .auto_handoff_on_sleep_enabled
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            AISettingsPageAction::ToggleAgentAttribution => {
                // The updated value syncs to warp-server automatically via
                // `CloudPreferencesSyncer` as a `JsonPreference` GSO keyed
                // `Global_AgentAttributionEnabled`; no bespoke server call needed.
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .agent_attribution_enabled
                        .toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
        }
    }
}

impl SettingsPageMeta for AISettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::AI
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        FeatureFlag::AgentMode.is_enabled()
    }

    fn on_page_selected(&mut self, _: bool, ctx: &mut ViewContext<Self>) {
        AIRequestUsageModel::handle(ctx).update(ctx, |ai_request_usage_model, ctx| {
            ai_request_usage_model.refresh_request_usage_async(ctx)
        });
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<AISettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AISettingsPageView>) -> Self {
        SettingsPageViewHandle::AI(view_handle)
    }
}

fn render_ai_setting_toggle<S: Setting>(
    label: impl Into<String>,
    action: AISettingsPageAction,
    is_setting_enabled: bool,
    is_setting_toggleable: bool,
    switch_state: SwitchStateHandle,
    tooltip_states: &RefCell<HashMap<String, MouseStateHandle>>,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    build_toggle_element(
        render_body_item_label::<AISettingsPageAction>(
            label.into(),
            Some(styles::header_font_color(is_setting_toggleable, app)),
            None,
            LocalOnlyIconState::for_setting(
                S::storage_key(),
                S::sync_to_cloud(),
                &mut tooltip_states.borrow_mut(),
                app,
            ),
            ToggleState::Enabled,
            appearance,
        ),
        render_ai_feature_switch(
            switch_state,
            is_setting_enabled,
            is_setting_toggleable,
            action,
            app,
        ),
        appearance,
        None,
    )
}

fn render_ai_setting_label<S: Setting>(
    label: impl Into<String>,
    is_setting_toggleable: bool,
    tooltip_states: &RefCell<HashMap<String, MouseStateHandle>>,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    Container::new(render_body_item_label::<AISettingsPageAction>(
        label.into(),
        Some(styles::header_font_color(is_setting_toggleable, app)),
        None,
        LocalOnlyIconState::for_setting(
            S::storage_key(),
            S::sync_to_cloud(),
            &mut tooltip_states.borrow_mut(),
            app,
        ),
        ToggleState::Enabled,
        appearance,
    ))
    .with_margin_bottom(HEADER_PADDING)
    .finish()
}

fn render_ai_setting_description(
    description: impl Into<Cow<'static, str>>,
    is_setting_toggleable: bool,
    app: &AppContext,
) -> Box<dyn Element> {
    let default_font_size = Appearance::as_ref(app).ui_font_size();
    render_ai_setting_description_with_font_size(
        description,
        default_font_size,
        is_setting_toggleable,
        app,
    )
}

fn render_ai_setting_description_with_font_size(
    description: impl Into<Cow<'static, str>>,
    font_size: f32,
    is_setting_toggleable: bool,
    app: &AppContext,
) -> Box<dyn Element> {
    let ui_builder = Appearance::as_ref(app).ui_builder();
    ui_builder
        .paragraph(description)
        .with_style(UiComponentStyles {
            font_size: Some(font_size),
            font_color: Some(styles::description_font_color(is_setting_toggleable, app).into()),
            margin: Some(
                Coords::default()
                    .top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                    .bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                    .right(styles::TOGGLE_WIDTH_MARGIN),
            ),
            ..Default::default()
        })
        .build()
        .finish()
}

fn render_toolbar_layout_editor(
    editor: &ViewHandle<AgentToolbarInlineEditor>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let label = Container::new(
        appearance
            .ui_builder()
            .span("Toolbar layout".to_string())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_margin_bottom(4.)
    .finish();
    let editor = Container::new(ChildView::new(editor).finish())
        .with_margin_bottom(16.)
        .finish();

    Flex::column().with_child(label).with_child(editor).finish()
}

fn render_ai_feature_switch(
    state_handle: SwitchStateHandle,
    is_setting_enabled: bool,
    is_setting_toggleable: bool,
    toggle_action: AISettingsPageAction,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let ui_builder = appearance.ui_builder();
    ui_builder
        .switch(state_handle)
        .check(is_setting_enabled)
        .with_disabled(!is_setting_toggleable)
        .with_disabled_styles(UiComponentStyles {
            background: Some(Fill::Solid(internal_colors::neutral_4(appearance.theme()))),
            foreground: Some(Fill::Solid(internal_colors::neutral_5(appearance.theme()))),
            ..Default::default()
        })
        .build()
        .on_click(move |ctx, _, _| {
            if !is_setting_toggleable {
                return;
            }
            ctx.dispatch_typed_action(toggle_action.clone());
        })
        .finish()
}

fn render_ai_list(
    header: &str,
    description: &str,
    input_list: Box<dyn Element>,
    view: &AISettingsPageView,
    ai_settings: &AISettings,
    app: &AppContext,
) -> Box<dyn Element> {
    let setting_header = render_ai_setting_label::<AgentModeCommandExecutionDenylist>(
        header.to_string(),
        ai_settings.is_any_ai_enabled(app),
        &view.local_only_icon_tooltip_states,
        app,
    );

    let description = render_ai_setting_description(
        description.to_string(),
        ai_settings.is_any_ai_enabled(app),
        app,
    );

    Flex::column()
        .with_child(setting_header)
        .with_child(Container::new(description).with_margin_bottom(-8.).finish())
        .with_child(input_list)
        .finish()
}

#[derive(Default)]
struct GlobalAIWidget {
    switch_state: SwitchStateHandle,
    sign_up_button: MouseStateHandle,
}

impl SettingsWidget for GlobalAIWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "oz warp agent global ai a.i. active next command prompt code diffs suggestion suggested suggestions \
                agent mode natural language detection input hint api keys bring your own byo google anthropic openai"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ui_builder = appearance.ui_builder();
        let is_ai_disabled_due_to_remote_session_org_policy =
            AISettings::as_ref(app).is_ai_disabled_due_to_remote_session_org_policy(app);

        let is_anonymous = AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out();

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(
                    "Warp Agent",
                    appearance.ui_font_family(),
                    PRIMARY_HEADER_FONT_SIZE,
                )
                .with_style(Properties::default().weight(Weight::Bold))
                .with_color(appearance.theme().active_ui_text_color().into())
                .finish(),
            );

        if is_ai_disabled_due_to_remote_session_org_policy {
            row.add_child(
                ConstrainedBox::new(
                    Container::new(
                        Text::new("Your organization disallows AI when the active pane contains content from a remote session", appearance.ui_font_family(), 12.)
                            .with_color(appearance.theme().ui_warning_color())
                            .finish()
                    )
                    .with_padding_left(8.)
                    .with_padding_right(8.)
                    .finish()
                )
                .with_max_width(400.)
                .finish()
            );
        }

        // Show sign-up button for anonymous users, toggle for logged-in users
        if is_anonymous {
            row.add_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            Text::new_inline(
                                "To use AI features, please create an account.",
                                appearance.ui_font_family(),
                                14.,
                            )
                            .with_color(
                                appearance
                                    .theme()
                                    .sub_text_color(appearance.theme().surface_2())
                                    .into_solid(),
                            )
                            .finish(),
                        )
                        .with_margin_right(16.)
                        .finish(),
                    )
                    .with_child(
                        Container::new(
                            ui_builder
                                .button(ButtonVariant::Accent, self.sign_up_button.clone())
                                .with_style(UiComponentStyles {
                                    font_size: Some(14.),
                                    font_weight: Some(Weight::Semibold),
                                    border_radius: Some(CornerRadius::with_all(Radius::Pixels(4.))),
                                    padding: Some(Coords {
                                        top: 8.,
                                        bottom: 8.,
                                        left: 24.,
                                        right: 24.,
                                    }),
                                    ..Default::default()
                                })
                                .with_text_label("Sign up".to_owned())
                                .build()
                                .on_click(move |ctx, _, _| {
                                    ctx.dispatch_typed_action(
                                        AISettingsPageAction::SignupAnonymousUser,
                                    );
                                })
                                .finish(),
                        )
                        .with_padding_right(TOGGLE_BUTTON_RIGHT_PADDING)
                        .finish(),
                    )
                    .finish(),
            );
        } else {
            row.add_child(
                Container::new(
                    ui_builder
                        .switch(self.switch_state.clone())
                        .check(AISettings::as_ref(app).is_any_ai_enabled(app))
                        .build()
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(AISettingsPageAction::ToggleGlobalAI);
                        })
                        .finish(),
                )
                .with_padding_right(TOGGLE_BUTTON_RIGHT_PADDING)
                .finish(),
            );
        }

        Container::new(row.finish())
            .with_padding_bottom(15.)
            .finish()
    }
}

#[derive(Default)]
struct UsageWidget {
    requests_highlight_index: HighlightedHyperlink,
}

impl UsageWidget {
    fn render_request_usage_count(
        &self,
        used: usize,
        limit: usize,
        is_unlimited: bool,
        workspace_is_delinquent_due_to_payment_issue: bool,
        appearance: &Appearance,
    ) -> Box<dyn warpui::Element> {
        let mut row = Flex::row();
        if used >= limit || workspace_is_delinquent_due_to_payment_issue {
            row.add_child(
                ConstrainedBox::new(
                    Icon::AlertTriangle
                        .to_warpui_icon(appearance.theme().ui_error_color().into())
                        .finish(),
                )
                .with_height(16.)
                .with_width(16.)
                .finish(),
            )
        }

        let request_count_label = if workspace_is_delinquent_due_to_payment_issue {
            "Restricted due to billing issue".to_string()
        } else if is_unlimited {
            "Unlimited".to_string()
        } else {
            format!("{used}/{limit}")
        };

        row.add_child(
            appearance
                .ui_builder()
                .paragraph(request_count_label)
                .with_style(UiComponentStyles {
                    font_color: {
                        if used >= limit {
                            Some(appearance.theme().ui_error_color())
                        } else {
                            Some(blended_colors::text_sub(
                                appearance.theme(),
                                appearance.theme().surface_1(),
                            ))
                        }
                    },
                    font_size: Some(16.),
                    margin: Some(Coords {
                        top: 0.,
                        bottom: 0.,
                        left: 8.,
                        right: 0.,
                    }),
                    ..Default::default()
                })
                .build()
                .finish(),
        );

        row.finish()
    }

    /// Renders a row of what is being limited, along with the current used/limit.
    #[allow(clippy::too_many_arguments)]
    fn render_ai_usage_limit_row(
        &self,
        header: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
        used: usize,
        limit: usize,
        is_unlimited: bool,
        workspace_is_delinquent_due_to_payment_issue: bool,
        appearance: &Appearance,
    ) -> Box<dyn warpui::Element> {
        let request_usage_details = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::End)
            .with_child(self.render_request_usage_count(
                used,
                limit,
                is_unlimited,
                workspace_is_delinquent_due_to_payment_issue,
                appearance,
            ));

        let request_usage_description = FormattedTextElement::from_str(
            description,
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_color(blended_colors::text_sub(
            appearance.theme(),
            appearance.theme().surface_1(),
        ));

        Flex::row()
            .with_child(
                Shrinkable::new(
                    2.,
                    Container::new(
                        Flex::column()
                            .with_child(
                                appearance
                                    .ui_builder()
                                    .paragraph(header)
                                    .with_style(UiComponentStyles {
                                        font_color: Some(blended_colors::text_main(
                                            appearance.theme(),
                                            appearance.theme().surface_1(),
                                        )),
                                        margin: Some(Coords {
                                            top: 0.,
                                            bottom: 4.,
                                            left: 0.,
                                            right: 0.,
                                        }),
                                        ..Default::default()
                                    })
                                    .build()
                                    .finish(),
                            )
                            .with_child(request_usage_description.finish())
                            .finish(),
                    )
                    .with_margin_bottom(16.)
                    .finish(),
                )
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.,
                    Container::new(request_usage_details.finish())
                        .with_margin_bottom(16.)
                        .finish(),
                )
                .finish(),
            )
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_main_axis_size(MainAxisSize::Max)
            .finish()
    }
}

impl SettingsWidget for UsageWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "a.i. ai usage limit plan"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_request_usage_model = AIRequestUsageModel::as_ref(app);
        let next_refresh_time = ai_request_usage_model.next_refresh_time();
        let formatted_next_refresh_time = next_refresh_time.format("%b %d").to_string();
        let workspace_is_delinquent_due_to_payment_issue = UserWorkspaces::as_ref(app)
            .current_team()
            .map(|team| team.billing_metadata.is_delinquent_due_to_payment_issue())
            .unwrap_or_default();

        let usage_header = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    build_sub_header(
                        appearance,
                        "Usage",
                        Some(styles::header_font_color(true, app)),
                    )
                    .finish(),
                )
                .with_child(
                    appearance
                        .ui_builder()
                        .paragraph(format!("Resets {formatted_next_refresh_time}"))
                        .with_style(UiComponentStyles {
                            font_color: Some(blended_colors::text_sub(
                                appearance.theme(),
                                appearance.theme().surface_1(),
                            )),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .finish(),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();

        let request_limit_description = format!(
            "This is the {} limit of AI credits for your account.",
            ai_request_usage_model.refresh_duration_to_string()
        );

        let request_usage_row = self.render_ai_usage_limit_row(
            "Credits",
            request_limit_description,
            ai_request_usage_model.requests_used(),
            ai_request_usage_model.request_limit(),
            ai_request_usage_model.is_unlimited(),
            workspace_is_delinquent_due_to_payment_issue,
            appearance,
        );

        let auth_state = AuthStateProvider::as_ref(app).get();
        let upgrade_cta_text_fragments = if let Some(team) =
            UserWorkspaces::as_ref(app).current_team()
        {
            let current_user_email = auth_state.user_email().unwrap_or_default();
            let has_admin_permissions = team.has_admin_permissions(&current_user_email);
            if team.billing_metadata.can_upgrade_to_higher_tier_plan() {
                let upgrade_url = UserWorkspaces::upgrade_link_for_team(team.uid);
                if has_admin_permissions {
                    vec![
                        FormattedTextFragment::hyperlink("Upgrade", upgrade_url),
                        FormattedTextFragment::plain_text(" to get more AI usage."),
                    ]
                } else {
                    // The /upgrade page says to contact their administrator.
                    vec![
                        FormattedTextFragment::hyperlink("Compare plans", upgrade_url),
                        FormattedTextFragment::plain_text(" for more AI usage."),
                    ]
                }
            } else {
                vec![
                    FormattedTextFragment::hyperlink("Contact support", "mailto:support@warp.dev"),
                    FormattedTextFragment::plain_text(" for more AI usage."),
                ]
            }
        } else {
            let user_id = auth_state.user_id().unwrap_or_default();
            let upgrade_url = UserWorkspaces::upgrade_link(user_id);
            vec![
                FormattedTextFragment::hyperlink("Upgrade", upgrade_url),
                FormattedTextFragment::plain_text(" to get more AI usage."),
            ]
        };

        let mut upgrade_cta = FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(upgrade_cta_text_fragments)]),
            appearance.ui_font_size(),
            appearance.ui_font_family(),
            appearance.ui_font_family(),
            styles::description_font_color(true, app).into(),
            self.requests_highlight_index.clone(),
        )
        .with_hyperlink_font_color(appearance.theme().accent().into_solid());

        if AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
        {
            upgrade_cta = upgrade_cta.register_default_click_handlers(|_, ctx, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::AttemptLoginGatedUpgrade);
            });
        } else {
            upgrade_cta = upgrade_cta.register_default_click_handlers(|url, ctx, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
            })
        }

        Flex::column()
            .with_children([
                render_separator(appearance),
                usage_header,
                request_usage_row,
                Container::new(upgrade_cta.finish())
                    .with_margin_bottom(16.)
                    .finish(),
            ])
            .finish()
    }
}

#[derive(Default)]
struct ActiveAIWidget {
    active_ai_toggle: SwitchStateHandle,
    intelligent_autosuggestions_toggle: SwitchStateHandle,
    prompt_suggestions_toggle: SwitchStateHandle,
    code_suggestions_toggle: SwitchStateHandle,
    natural_language_autosuggestions_toggle: SwitchStateHandle,
    shared_block_title_generation_toggle: SwitchStateHandle,
    git_operations_autogen_toggle: SwitchStateHandle,
}

impl ActiveAIWidget {
    fn is_next_command_toggleable(&self, app: &AppContext) -> bool {
        UserWorkspaces::as_ref(app).is_next_command_enabled()
            && AISettings::as_ref(app)
                .intelligent_autosuggestions_enabled_internal
                .is_supported_on_current_platform()
    }

    fn is_prompt_suggestions_toggleable(&self, app: &AppContext) -> bool {
        UserWorkspaces::as_ref(app).is_prompt_suggestions_toggleable()
            && AISettings::as_ref(app)
                .prompt_suggestions_enabled_internal
                .is_supported_on_current_platform()
    }

    fn is_suggested_code_banners_toggleable(&self, app: &AppContext) -> bool {
        (self.is_prompt_suggestions_toggleable(app)
            || UserWorkspaces::as_ref(app).is_code_suggestions_toggleable())
            && AISettings::as_ref(app)
                .code_suggestions_enabled_internal
                .is_supported_on_current_platform()
    }

    fn is_natural_language_autosuggestions_toggleable(&self, app: &AppContext) -> bool {
        FeatureFlag::PredictAMQueries.is_enabled()
            && AISettings::as_ref(app)
                .natural_language_autosuggestions_enabled_internal
                .is_supported_on_current_platform()
    }

    // TODO: Check if the user's enterprise billing policy allows toggling this feature.
    fn is_shared_block_title_generation_toggleable(&self, app: &AppContext) -> bool {
        FeatureFlag::SharedBlockTitleGeneration.is_enabled()
            && AISettings::as_ref(app)
                .shared_block_title_generation_enabled_internal
                .is_supported_on_current_platform()
            && (!UserWorkspaces::as_ref(app)
                .current_team()
                .is_some_and(|team| {
                    team.billing_metadata.customer_type == CustomerType::Enterprise
                })
                // Override the enterprise check for dogfood builds, as our dogfood team
                // is an enterprise team.
                || ChannelState::channel().is_dogfood())
    }

    fn is_git_operations_autogen_toggleable(&self, app: &AppContext) -> bool {
        FeatureFlag::GitOperationsInCodeReview.is_enabled()
            && AISettings::as_ref(app)
                .git_operations_autogen_enabled_internal
                .is_supported_on_current_platform()
            && UserWorkspaces::as_ref(app).is_git_operations_ai_enabled()
    }

    fn render_next_command_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);

        Flex::column()
            .with_child(
                render_ai_setting_toggle::<IntelligentAutosuggestionsEnabled>(
                    "Next Command",
                    AISettingsPageAction::ToggleIntelligentAutosuggestions,
                    *ai_settings.intelligent_autosuggestions_enabled_internal,
                    is_toggleable,
                    self.intelligent_autosuggestions_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
            )
            .with_child(render_ai_setting_description(
                NEXT_COMMAND_DESCRIPTION,
                is_toggleable,
                app,
            ))
            .finish()
    }

    fn render_prompt_suggestions_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);
        Flex::column()
            .with_child(
                render_ai_setting_toggle::<AgentModeQuerySuggestionsEnabled>(
                    "Prompt Suggestions",
                    AISettingsPageAction::TogglePromptSuggestions,
                    *ai_settings.prompt_suggestions_enabled_internal,
                    is_toggleable,
                    self.prompt_suggestions_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
            )
            .with_child(render_ai_setting_description(
                PROMPT_SUGGESTIONS_DESCRIPTION,
                is_toggleable,
                app,
            ))
            .finish()
    }

    fn render_suggested_code_banners_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);
        Flex::column()
            .with_child(
                render_ai_setting_toggle::<AgentModeQuerySuggestionsEnabled>(
                    "Suggested Code Banners",
                    AISettingsPageAction::ToggleCodeSuggestions,
                    *ai_settings.code_suggestions_enabled_internal,
                    is_toggleable,
                    self.code_suggestions_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
            )
            .with_child(render_ai_setting_description(
                SUGGESTED_CODE_BANNERS_DESCRIPTION,
                is_toggleable,
                app,
            ))
            .finish()
    }

    fn render_natural_language_autosuggestions_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);
        Flex::column()
            .with_child(render_ai_setting_toggle::<
                NaturalLanguageAutosuggestionsEnabled,
            >(
                "Natural Language Autosuggestions",
                AISettingsPageAction::ToggleNaturalLanguageAutosuggestions,
                *ai_settings.natural_language_autosuggestions_enabled_internal,
                is_toggleable,
                self.natural_language_autosuggestions_toggle.clone(),
                &view.local_only_icon_tooltip_states,
                app,
            ))
            .with_child(render_ai_setting_description(
                NATURAL_LANGUAGE_AUTOSUGGESTIONS,
                is_toggleable,
                app,
            ))
            .finish()
    }

    fn render_shared_block_title_generation_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);
        Flex::column()
            .with_child(
                render_ai_setting_toggle::<SharedBlockTitleGenerationEnabled>(
                    "Shared Block Title Generation",
                    AISettingsPageAction::ToggleSharedTitleGeneration,
                    *ai_settings.shared_block_title_generation_enabled_internal,
                    is_toggleable,
                    self.shared_block_title_generation_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
            )
            .with_child(render_ai_setting_description(
                SHARED_BLOCK_TITLE_GENERATION_DESCRIPTION,
                is_toggleable,
                app,
            ))
            .finish()
    }

    fn render_git_operations_autogen_section(
        &self,
        view: &AISettingsPageView,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_active_ai_enabled(app);
        Flex::column()
            .with_child(render_ai_setting_toggle::<GitOperationsAutogenEnabled>(
                "Commit & Pull Request Generation",
                AISettingsPageAction::ToggleGitOperationsAutogen,
                *ai_settings.git_operations_autogen_enabled_internal,
                is_toggleable,
                self.git_operations_autogen_toggle.clone(),
                &view.local_only_icon_tooltip_states,
                app,
            ))
            .with_child(render_ai_setting_description(
                GIT_OPERATIONS_AUTOGEN_DESCRIPTION,
                is_toggleable,
                app,
            ))
            .finish()
    }
}

impl SettingsWidget for ActiveAIWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "active ai a.i. next command prompt suggestions code diffs suggested banners passive unit tests commit pull request pr git code review autogen generate"
    }

    fn should_render(&self, app: &AppContext) -> bool {
        self.is_next_command_toggleable(app)
            || self.is_prompt_suggestions_toggleable(app)
            || self.is_suggested_code_banners_toggleable(app)
            || self.is_natural_language_autosuggestions_toggleable(app)
            || self.is_shared_block_title_generation_toggleable(app)
            || self.is_git_operations_autogen_toggleable(app)
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let mut column = Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                Container::new(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_child(
                            build_sub_header(
                                appearance,
                                "Active AI",
                                Some(styles::header_font_color(is_any_ai_enabled, app)),
                            )
                            .finish(),
                        )
                        .with_child(
                            Container::new(render_ai_feature_switch(
                                self.active_ai_toggle.clone(),
                                *ai_settings.is_active_ai_enabled_internal,
                                is_any_ai_enabled,
                                AISettingsPageAction::ToggleActiveAI,
                                app,
                            ))
                            .with_padding_right(TOGGLE_BUTTON_RIGHT_PADDING)
                            .finish(),
                        )
                        .finish(),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            );

        if self.is_next_command_toggleable(app) {
            column.add_child(self.render_next_command_section(view, app));
        }

        if self.is_prompt_suggestions_toggleable(app) {
            column.add_child(self.render_prompt_suggestions_section(view, app));
        }

        if self.is_suggested_code_banners_toggleable(app) {
            column.add_child(self.render_suggested_code_banners_section(view, app));
        }

        if self.is_natural_language_autosuggestions_toggleable(app) {
            column.add_child(self.render_natural_language_autosuggestions_section(view, app));
        }

        if self.is_shared_block_title_generation_toggleable(app) {
            column.add_child(self.render_shared_block_title_generation_section(view, app));
        }

        if self.is_git_operations_autogen_toggleable(app) {
            column.add_child(self.render_git_operations_autogen_section(view, app));
        }

        column.finish()
    }
}

#[derive(Default)]
struct AgentsWidget {
    codebase_context_toggle: SwitchStateHandle,
    codebase_context_link_index: HighlightedHyperlink,
    show_in_prompt_checkbox: MouseStateHandle,
}

impl SettingsWidget for AgentsWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        if MCPServersWidget::should_show_mcp() {
            "ai a.i. agent autonomy profiles allowlist denylist autoexecute permissions models llms planning mcp server"
        } else {
            "ai a.i. agent autonomy profiles allowlist denylist autoexecute permissions models llms planning"
        }
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let mut column = Flex::column();

        if FeatureFlag::ProfilesDesignRevamp.is_enabled() {
            column.add_child(
                Container::new(self.render_profiles_section(view, ai_settings, appearance, app))
                    .with_margin_bottom(8.)
                    .finish(),
            );
        } else {
            // Legacy layout: show Agents header + Models + Permissions
            let mut agents_header = Flex::column();
            agents_header.add_child(
                build_sub_header(
                    appearance,
                    "Agents",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            );
            agents_header.add_child(render_ai_setting_description(
                "Set the boundaries for how your Agent operates. Choose what it can access, how much autonomy it has, and when it must ask for your approval. You can also fine-tune behavior around natural language input, codebase awareness, and more.",
                ai_settings.is_any_ai_enabled(app),
                app,
            ));
            let agents_header = agents_header.finish();
            column.add_children([
                render_separator(appearance),
                Container::new(agents_header)
                    .with_margin_bottom(8.)
                    .finish(),
            ]);
            column.add_children([
                Container::new(self.render_models_section(view, ai_settings, appearance, app))
                    .with_margin_bottom(8.)
                    .finish(),
                Container::new(self.render_permissions_section(view, ai_settings, appearance, app))
                    .with_margin_bottom(8.)
                    .finish(),
            ]);
        };

        column.finish()
    }
}

impl AgentsWidget {
    fn render_profiles_section(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let header_and_description = Flex::column()
            .with_child(
                build_sub_header(
                    appearance,
                    "Profiles",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .finish(),
            )
            .with_child(
                Container::new(
                    render_ai_setting_description(
                        "Profiles let you define how your Agent operates — from the actions it can take and when it needs approval, to the models it uses for tasks like coding and planning. You can also scope them to individual projects.",
                        is_any_ai_enabled,
                        app,
                    )
                )
                .with_margin_top(12.)
                .finish()
            )
            .finish();

        let mut profiles_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Shrinkable::new(1., header_and_description).finish());

        if FeatureFlag::MultiProfile.is_enabled() {
            profiles_row.add_child(
                Container::new(view.add_profile_button.as_ref(app).render(app))
                    .with_margin_left(16.)
                    .finish(),
            );
        }

        let profiles_header = Container::new(profiles_row.finish())
            .with_margin_bottom(12.0)
            .finish();

        let mut profile_elements = vec![profiles_header];

        for profile_view in &view.profile_views {
            profile_elements.push(
                Container::new(ChildView::new(profile_view).finish())
                    .with_margin_bottom(8.)
                    .finish(),
            );
        }

        Flex::column().with_children(profile_elements).finish()
    }

    fn render_models_section(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let model_subheader = Container::new(render_custom_size_header(
            appearance,
            "Models",
            14.0,
            Some(styles::header_font_color(is_any_ai_enabled, app)),
        ))
        .with_margin_bottom(8.0)
        .finish();

        let base_model_setting =
            Container::new(self.render_base_model_setting(view, ai_settings, appearance, app))
                .with_margin_bottom(8.0)
                .finish();

        let mut children = vec![model_subheader, base_model_setting];
        if let Some(context_window_setting) =
            self.render_context_window_setting(view, ai_settings, appearance, app)
        {
            children.push(
                Container::new(context_window_setting)
                    .with_margin_bottom(8.0)
                    .finish(),
            );
        }

        Flex::column().with_children(children).finish()
    }

    /// Renders the context window slider + numeric input row shown below the
    /// base model dropdown. Returns `None` if the active base model does not
    /// advertise a configurable context window or global AI is disabled.
    fn render_context_window_setting(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !ai_settings.is_any_ai_enabled(app) {
            return None;
        }
        let cw = AISettingsPageView::configurable_context_window(app)?;
        let min = cw.min;
        let max = cw.max;

        let label = Container::new(render_body_item_label::<AISettingsPageAction>(
            "Context window (tokens)".to_string(),
            None,
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        ))
        .with_margin_bottom(4.0)
        .finish();

        let min_label = appearance
            .ui_builder()
            .span(format!("{min}"))
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish();

        let max_label = appearance
            .ui_builder()
            .span(format!("{max}"))
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish();

        let current_value = AISettingsPageView::current_context_window_display_value(app)
            .unwrap_or(cw.default_max)
            .clamp(min, max);
        let slider = appearance
            .ui_builder()
            .slider(view.context_window_slider_state.clone())
            .with_range(min as f32..max as f32)
            .with_default_value(current_value as f32)
            .with_style(UiComponentStyles {
                width: Some(CONTEXT_WINDOW_SLIDER_WIDTH),
                margin: Some(Coords::default().left(8.).right(8.)),
                ..Default::default()
            })
            .on_drag(|ctx, _, val| {
                ctx.dispatch_typed_action(AISettingsPageAction::ContextWindowSliderDragged(
                    val.round() as u32,
                ));
            })
            .on_change(|ctx, _, val| {
                ctx.dispatch_typed_action(AISettingsPageAction::SetContextWindowSize(
                    val.round() as u32
                ));
            })
            .build()
            .finish();

        let context_window_editor = view.context_window_editor.clone();
        let input_box = Dismiss::new(
            appearance
                .ui_builder()
                .text_input(view.context_window_editor.clone())
                .with_style(UiComponentStyles {
                    width: Some(CONTEXT_WINDOW_INPUT_BOX_WIDTH),
                    padding: Some(Coords {
                        top: 6.,
                        bottom: 6.,
                        left: 10.,
                        right: 10.,
                    }),
                    margin: Some(Coords::default().left(12.)),
                    background: Some(appearance.theme().surface_2().into()),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .on_dismiss(move |ctx, app| {
            let buffer_text = context_window_editor.as_ref(app).buffer_text(app);
            let cleaned: String = buffer_text
                .chars()
                .filter(|c| !c.is_whitespace() && *c != ',')
                .collect();
            if let Ok(parsed) = cleaned.parse::<u32>() {
                ctx.dispatch_typed_action(AISettingsPageAction::SetContextWindowSize(parsed));
            }
        })
        .finish();

        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(min_label)
            .with_child(Shrinkable::new(1., slider).finish())
            .with_child(max_label)
            .with_child(input_box)
            .finish();

        let mut column = Flex::column().with_child(label).with_child(row);
        if AISettingsPageView::active_profile_data(app)
            .should_show_long_context_pricing_warning(view.dragged_context_window_value, app)
        {
            column.add_child(render_warning_box(
                WarningBoxConfig::formatted_title(long_context_pricing_warning_title()),
                appearance,
            ));
        }

        Some(column.finish())
    }

    fn render_permissions_section(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let permissions_subheader = Container::new(render_custom_size_header(
            appearance,
            "Permissions",
            14.0,
            Some(styles::header_font_color(is_any_ai_enabled, app)),
        ))
        .with_margin_bottom(4.0)
        .finish();

        let code_diff_setting =
            BlocklistAIPermissions::as_ref(app).get_apply_code_diffs_setting(app, None);
        let code_diffs = self.render_execution_profile_dropdown(
            "Apply code diffs",
            Icon::Code2,
            code_diff_setting.description(),
            &view.apply_code_diffs_dropdown_menu,
            ai_settings,
            appearance,
            app,
        );

        let read_files_setting =
            BlocklistAIPermissions::as_ref(app).get_read_files_setting(app, None);
        let mut read_files_flex = Flex::column().with_main_axis_size(MainAxisSize::Min);
        read_files_flex.add_child(self.render_execution_profile_dropdown(
            "Read files",
            Icon::Notebook,
            read_files_setting.description(),
            &view.read_files_dropdown_menu,
            ai_settings,
            appearance,
            app,
        ));

        if read_files_setting == ActionPermission::AlwaysAsk {
            let directory_allowlist =
                BlocklistAIPermissions::as_ref(app).get_read_files_allowlist(app, None);
            read_files_flex.add_child(
                Container::new(Self::render_directory_allowlist(
                    directory_allowlist,
                    view,
                    ai_settings,
                    appearance,
                    app,
                ))
                .with_margin_bottom(HEADER_PADDING)
                .finish(),
            );
        }
        let read_files = read_files_flex.finish();

        let execute_commands_setting =
            BlocklistAIPermissions::as_ref(app).get_execute_commands_setting(app, None);
        let mut execute_commands_flex = Flex::column().with_main_axis_size(MainAxisSize::Min);
        execute_commands_flex.add_child(self.render_execution_profile_dropdown(
            "Execute commands",
            Icon::Terminal,
            execute_commands_setting.description(),
            &view.execute_commands_dropdown_menu,
            ai_settings,
            appearance,
            app,
        ));

        if execute_commands_setting == ActionPermission::AlwaysAsk
            || execute_commands_setting == ActionPermission::AgentDecides
        {
            let command_allowlist =
                BlocklistAIPermissions::as_ref(app).get_execute_commands_allowlist(app, None);
            execute_commands_flex.add_child(
                Container::new(Self::render_command_allowlist(
                    command_allowlist,
                    view,
                    ai_settings,
                    appearance,
                    app,
                ))
                .with_margin_bottom(8.)
                .finish(),
            );
        }

        if execute_commands_setting != ActionPermission::AlwaysAsk {
            let command_denylist = Container::new(Self::render_command_denylist(
                BlocklistAIPermissions::as_ref(app).get_execute_commands_denylist(app, None),
                view,
                ai_settings,
                appearance,
                app,
            ))
            .with_margin_bottom(8.)
            .finish();
            execute_commands_flex.add_child(command_denylist);
        }
        let execute_commands = execute_commands_flex.finish();

        let mut widget_children = vec![permissions_subheader];

        if UserWorkspaces::as_ref(app)
            .ai_autonomy_settings()
            .has_any_overrides()
        {
            widget_children.push(
                Container::new(render_settings_info_banner(
                    "Some of your permissions are managed by your workspace.",
                    None,
                    appearance,
                ))
                .with_margin_bottom(12.0)
                .finish(),
            );
        }

        widget_children.extend([code_diffs, read_files, execute_commands]);

        let write_to_pty_setting =
            BlocklistAIPermissions::as_ref(app).get_write_to_pty_setting(app, None);
        let write_to_pty = self.render_execution_profile_dropdown(
            "Interact with running commands",
            Icon::Workflow,
            write_to_pty_setting.description(),
            &view.write_to_pty_autonomy_dropdown_menu,
            ai_settings,
            appearance,
            app,
        );
        widget_children.push(write_to_pty);

        if MCPServersWidget::should_show_mcp() {
            let mcp_permissions = self.render_mcp_permissions(view, ai_settings, appearance, app);
            widget_children.push(mcp_permissions);
        }

        if !FeatureFlag::FullSourceCodeEmbedding.is_enabled() {
            let codebase_context = Self::render_codebase_context_outline_generation_setting(
                self.codebase_context_toggle.clone(),
                self.codebase_context_link_index.clone(),
                view,
                ai_settings,
                appearance,
                app,
            );
            widget_children.push(codebase_context);
        }

        Flex::column().with_children(widget_children).finish()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_execution_profile_dropdown(
        &self,
        header_text: &str,
        header_icon: Icon,
        permission_description: &'static str,
        dropdown_menu: &ViewHandle<Dropdown<AISettingsPageAction>>,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &warpui::AppContext,
    ) -> Box<dyn Element> {
        let header = Container::new(render_body_item_label_with_icon::<AISettingsPageAction>(
            header_text.into(),
            header_icon,
            Some(styles::header_font_color(
                ai_settings.is_any_ai_enabled(app),
                app,
            )),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        ))
        .finish();

        let description_color = appearance.theme().disabled_ui_text_color();
        let alert_icon = Container::new(
            ConstrainedBox::new(
                Icon::AlertCircle
                    .to_warpui_icon(
                        appearance
                            .theme()
                            .sub_text_color(appearance.theme().surface_2()),
                    )
                    .finish(),
            )
            .with_width(14.)
            .with_height(14.)
            .finish(),
        )
        .with_margin_right(4.)
        .finish();
        let text = Text::new(
            permission_description,
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_color(description_color.into())
        .finish();
        let description = Flex::row()
            .with_children([alert_icon, Shrinkable::new(1.0, text).finish()])
            .finish();

        Container::new(
            Flex::column()
                .with_child(header)
                .with_child(ChildView::new(dropdown_menu).finish())
                .with_child(description)
                .finish(),
        )
        .with_margin_bottom(12.)
        .finish()
    }

    fn render_command_denylist(
        command_denylist: Vec<AgentModeCommandExecutionPredicate>,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_disabled = !ai_settings.is_any_ai_enabled(app);
        let org_denylist = BlocklistAIPermissions::get_org_execute_commands_denylist(app);
        let mut tooltip_idx = 0usize;
        let list = render_input_list(
            None,
            command_denylist
                .into_iter()
                .zip(view.command_denylist_mouse_state_handles.clone())
                .rev()
                .map(|(cmd, mouse_state_handle)| {
                    let is_org = org_denylist.contains(&cmd);
                    let tooltip_mouse_state = if is_org {
                        let handle = view
                            .command_denylist_tooltip_mouse_state_handles
                            .get(tooltip_idx)
                            .cloned();
                        tooltip_idx += 1;
                        handle
                    } else {
                        None
                    };
                    InputListItem {
                        item: cmd.to_string(),
                        mouse_state_handle,
                        on_remove_action: AISettingsPageAction::RemoveFromProfileCommandDenylist(
                            cmd,
                        ),
                        is_disabled: is_org || ai_disabled,
                        tooltip_mouse_state,
                    }
                }),
            Some(&view.command_denylist_editor),
            appearance,
        );
        render_ai_list(
            "Command denylist",
            "Regular expressions to match commands that the Warp Agent should always ask permission to execute.",
            list,
            view,
            ai_settings,
            app,
        )
    }

    fn render_command_allowlist(
        command_allowlist: Vec<AgentModeCommandExecutionPredicate>,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let disabled = !ai_settings.is_command_allowlist_editable(app);
        let list = render_input_list(
            None,
            command_allowlist
                .into_iter()
                .zip(view.command_allowlist_mouse_state_handles.clone())
                .rev()
                .map(move |(cmd, mouse_state_handle)| InputListItem {
                    item: cmd.to_string(),
                    mouse_state_handle,
                    on_remove_action: AISettingsPageAction::RemoveFromProfileCommandAllowlist(cmd),
                    is_disabled: disabled,
                    tooltip_mouse_state: None,
                }),
            Some(&view.command_allowlist_editor),
            appearance,
        );

        render_ai_list(
            "Command allowlist",
            "Regular expressions to match commands that can be automatically executed by the Warp Agent.",
            list,
            view,
            ai_settings,
            app,
        )
    }

    fn render_directory_allowlist(
        directory_allowlist: Vec<PathBuf>,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let disabled = !ai_settings.is_directory_allowlist_editable(app);
        let list = render_input_list(
            None,
            directory_allowlist
                .clone()
                .into_iter()
                .zip(view.directory_allowlist_mouse_state_handles.clone())
                .rev()
                .map(move |(path, mouse_state_handle)| InputListItem {
                    item: path.display().to_string(),
                    mouse_state_handle,
                    on_remove_action: AISettingsPageAction::RemoveFromProfileDirectoryAllowlist(
                        path,
                    ),
                    is_disabled: disabled,
                    tooltip_mouse_state: None,
                }),
            Some(&view.directory_allowlist_editor),
            appearance,
        );

        render_ai_list(
            "Directory allowlist",
            "Give the agent file access to certain directories.",
            list,
            view,
            ai_settings,
            app,
        )
    }

    fn render_base_model_setting(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let show_in_prompt_checkbox = {
            let is_checked = *SessionSettings::as_ref(app).show_model_selectors_in_prompt;

            let mut checkbox = appearance
                .ui_builder()
                .checkbox(self.show_in_prompt_checkbox.clone(), None)
                .check(is_checked);

            if !ai_settings.is_any_ai_enabled(app) {
                checkbox = checkbox.disabled();
            }

            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_children([
                        checkbox
                            .build()
                            .on_click(move |ctx, _, _| {
                                ctx.dispatch_typed_action(
                                    AISettingsPageAction::ToggleShowBaseModelPickerInPrompt,
                                );
                            })
                            .finish(),
                        appearance
                            .ui_builder()
                            .span("Show model picker in prompt".to_string())
                            .with_style(UiComponentStyles {
                                font_color: Some(
                                    theme.sub_text_color(theme.surface_2()).into_solid(),
                                ),
                                font_size: Some(12.0),
                                ..Default::default()
                            })
                            .build()
                            .finish(),
                    ])
                    .finish(),
            )
            .with_margin_top(-6.0)
            .with_margin_left(-4.0)
            .finish()
        };

        render_dropdown_item(
            appearance,
            "Base model",
            Some(
                "This model serves as the primary engine behind the Warp Agent. It powers most interactions and invokes other models for tasks like planning or code generation when necessary. Warp may automatically switch to alternate models based on model availability or for auxiliary tasks such as conversation summarization.",
            ),
            Some(show_in_prompt_checkbox),
            LocalOnlyIconState::Hidden,
            (!ai_settings.is_any_ai_enabled(app))
                .then(|| appearance.theme().disabled_ui_text_color()),
            &view.base_model_dropdown,
        )
    }

    fn render_codebase_context_outline_generation_setting(
        codebase_context_toggle: SwitchStateHandle,
        codebase_context_link_index: HighlightedHyperlink,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &warpui::AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);
        let toggle = render_ai_setting_toggle::<CodebaseContextEnabled>(
            "Codebase Context",
            AISettingsPageAction::ToggleCodebaseContext,
            *code_settings.codebase_context_enabled,
            ai_settings.is_any_ai_enabled(app),
            codebase_context_toggle,
            &view.local_only_icon_tooltip_states,
            app,
        );

        let codebase_context_description = vec![
            FormattedTextFragment::plain_text(
                "Allow the Warp Agent to generate an outline of your codebase that can be used for context. No code is ever stored on our servers. ",
            ),
            FormattedTextFragment::hyperlink(
                "Learn more",
                "https://docs.warp.dev/agent-platform/capabilities/codebase-context",
            ),
        ];
        let description = Container::new(
            FormattedTextElement::new(
                FormattedText::new([FormattedTextLine::Line(codebase_context_description)]),
                CONTENT_FONT_SIZE,
                appearance.ui_font_family(),
                appearance.ui_font_family(),
                styles::description_font_color(ai_settings.is_any_ai_enabled(app), app).into(),
                codebase_context_link_index,
            )
            .with_hyperlink_font_color(appearance.theme().accent().into_solid())
            .register_default_click_handlers(|url, ctx, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
            })
            .finish(),
        )
        .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
        .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
        .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
        .finish();

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }

    fn render_mcp_permissions(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let all_runnable_mcp_servers =
            TemplatableMCPServerManager::get_all_cloud_synced_mcp_servers(app);
        if all_runnable_mcp_servers.is_empty() {
            self.render_mcp_permissions_zero_state(ai_settings, appearance, app)
        } else {
            self.render_mcp_permissions_with_servers(view, ai_settings, appearance, app)
        }
    }

    fn render_mcp_permissions_zero_state(
        &self,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let header = Container::new(render_body_item_label_with_icon::<AISettingsPageAction>(
            "Call MCP servers".into(),
            Icon::Dataflow,
            Some(styles::header_font_color(
                ai_settings.is_any_ai_enabled(app),
                app,
            )),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        ))
        .with_margin_bottom(4.)
        .finish();

        let subtext = {
            let subtext_fragments = vec![
                FormattedTextFragment::plain_text(
                    "You haven't added any MCP servers yet. Once you do, you'll be able to control how much autonomy the Warp Agent has when interacting with them. ",
                ),
                FormattedTextFragment::hyperlink_action(
                    "Add a server",
                    AISettingsPageAction::OpenMCPServerCollection,
                ),
                FormattedTextFragment::plain_text(" or "),
                FormattedTextFragment::hyperlink(
                    "learn more about MCPs.",
                    "https://docs.warp.dev/agent-platform/capabilities/mcp",
                ),
            ];

            Container::new(
                FormattedTextElement::new(
                    FormattedText::new([FormattedTextLine::Line(subtext_fragments)]),
                    CONTENT_FONT_SIZE,
                    appearance.ui_font_family(),
                    appearance.ui_font_family(),
                    styles::description_font_color(ai_settings.is_any_ai_enabled(app), app).into(),
                    HighlightedHyperlink::default(),
                )
                .with_hyperlink_font_color(appearance.theme().accent().into_solid())
                .register_default_click_handlers_with_action_support(|hyperlink_lens, ctx, _app| {
                    match hyperlink_lens {
                        HyperlinkLens::Url(url) => {
                            ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(
                                HyperlinkUrl {
                                    url: url.to_owned(),
                                },
                            ));
                        }
                        HyperlinkLens::Action(action_ref) => {
                            if let Some(action) =
                                action_ref.as_any().downcast_ref::<AISettingsPageAction>()
                            {
                                ctx.dispatch_typed_action(action.clone());
                            }
                        }
                    }
                })
                .finish(),
            )
            .with_margin_bottom(4.0)
            .finish()
        };

        Flex::column()
            .with_child(header)
            .with_child(subtext)
            .finish()
    }

    fn render_mcp_permissions_with_servers(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let mut column = Flex::column();

        let current_mcp_setting =
            BlocklistAIPermissions::as_ref(app).get_mcp_permissions_setting(app, None);

        let permission_setting = self.render_execution_profile_dropdown(
            "Call MCP servers",
            Icon::Dataflow,
            current_mcp_setting.description(),
            &view.mcp_permissions_dropdown_menu,
            ai_settings,
            appearance,
            app,
        );
        column.add_child(permission_setting);

        if current_mcp_setting == ActionPermission::AlwaysAsk
            || current_mcp_setting == ActionPermission::AgentDecides
        {
            let allowlist = self.render_mcp_list(
                "MCP allowlist",
                "Allow the Warp Agent to call these MCP servers.",
                &view.mcp_allowlist_dropdown,
                BlocklistAIPermissions::as_ref(app).get_mcp_allowlist(app, None),
                view.mcp_allowlist_mouse_state_handles.clone(),
                AISettingsPageAction::RemoveFromMCPAllowlist,
                ai_settings,
                appearance,
                app,
            );
            column.add_child(allowlist);
        }

        if current_mcp_setting == ActionPermission::AlwaysAllow
            || current_mcp_setting == ActionPermission::AgentDecides
        {
            let denylist = self.render_mcp_list(
                "MCP denylist",
                "The Warp Agent will always ask for permission before calling any MCP servers on this list.",
                &view.mcp_denylist_dropdown,
                BlocklistAIPermissions::as_ref(app).get_mcp_denylist(app, None),
                view.mcp_denylist_mouse_state_handles.clone(),
                AISettingsPageAction::RemoveFromMCPDenylist,
                ai_settings,
                appearance,
                app,
            );
            column.add_child(denylist);
        }

        column.finish()
    }

    // Helper function to render the allow and denylists for mcp servers
    #[allow(clippy::too_many_arguments)]
    fn render_mcp_list(
        &self,
        title: &str,
        description: &str,
        dropdown: &ViewHandle<FilterableDropdown<AISettingsPageAction>>,
        items: Vec<uuid::Uuid>,
        mouse_state_handles: Vec<MouseStateHandle>,
        action: impl Fn(uuid::Uuid) -> AISettingsPageAction,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let selector = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_children(vec![
                    Shrinkable::new(
                        1.0,
                        Container::new(render_dropdown_item_label(
                            title.to_string(),
                            Some(description.to_string()),
                            LocalOnlyIconState::Hidden,
                            (!ai_settings.is_any_ai_enabled(app))
                                .then(|| appearance.theme().disabled_ui_text_color()),
                            appearance,
                        ))
                        .with_margin_right(4.)
                        .finish(),
                    )
                    .finish(),
                    ChildView::new(dropdown).finish(),
                ])
                .finish(),
        )
        .with_margin_bottom(2.)
        .finish();

        let disabled = !ai_settings.is_any_ai_enabled(app);
        let items = render_input_list(
            None,
            items
                .into_iter()
                .rev()
                .zip(mouse_state_handles.clone())
                .filter_map(move |(uuid, mouse_state_handle)| {
                    let server_name = TemplatableMCPServerManager::get_mcp_name(&uuid, app);
                    server_name.map(|server_name| InputListItem {
                        item: server_name,
                        mouse_state_handle,
                        on_remove_action: action(uuid),
                        is_disabled: disabled,
                        tooltip_mouse_state: None,
                    })
                }),
            None,
            appearance,
        );

        Container::new(Flex::column().with_children(vec![selector, items]).finish())
            .with_margin_bottom(8.)
            .finish()
    }
}

#[derive(Default)]
struct AIInputWidget {
    incorrect_autodetection_highlight_index: HighlightedHyperlink,
    autodetection_toggle: SwitchStateHandle,
    nld_in_terminal_toggle: SwitchStateHandle,
    show_input_hint_toggle: SwitchStateHandle,
    show_agent_tips_toggle: SwitchStateHandle,
    include_agent_commands_in_history_toggle: SwitchStateHandle,
}

impl SettingsWidget for AIInputWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "oz agent ai input natural language detection autodetection prompt terminal command commands history shell executed execution queue interrupt submission submit auto-queue response while responding default"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let input_header = build_sub_header(
            appearance,
            "Input",
            Some(styles::header_font_color(is_any_ai_enabled, app)),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();

        let natural_language_detection_section = Self::render_natural_language_detection_section(
            self.incorrect_autodetection_highlight_index.clone(),
            self.autodetection_toggle.clone(),
            self.nld_in_terminal_toggle.clone(),
            view,
            ai_settings,
            appearance,
            app,
        );

        let show_input_hint_text = render_ai_setting_toggle::<ShowHintText>(
            "Show input hint text",
            AISettingsPageAction::ToggleShowInputHintText,
            *InputSettings::as_ref(app).show_hint_text,
            is_any_ai_enabled,
            self.show_input_hint_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let mut widget_children = vec![
            render_separator(appearance),
            input_header,
            natural_language_detection_section,
            show_input_hint_text,
        ];

        if FeatureFlag::AgentTips.is_enabled() {
            let agent_tips_toggle = render_ai_setting_toggle::<ShowAgentTips>(
                "Show agent tips",
                AISettingsPageAction::ToggleShowAgentTips,
                *InputSettings::as_ref(app).show_agent_tips,
                is_any_ai_enabled,
                self.show_agent_tips_toggle.clone(),
                &view.local_only_icon_tooltip_states,
                app,
            );
            widget_children.push(agent_tips_toggle);
        }

        widget_children.push(render_ai_setting_toggle::<IncludeAgentCommandsInHistory>(
            "Include agent-executed commands in history",
            AISettingsPageAction::ToggleIncludeAgentCommandsInHistory,
            *ai_settings.include_agent_commands_in_history,
            is_any_ai_enabled,
            self.include_agent_commands_in_history_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        ));

        if FeatureFlag::QueueSlashCommand.is_enabled() {
            widget_children.push(render_dropdown_item(
                appearance,
                "Default prompt submission mode",
                Some(
                    "What happens when you submit a new prompt while the agent is still \
                     responding. You can override this per conversation using the auto-queue \
                     toggle.",
                ),
                None,
                LocalOnlyIconState::for_setting(
                    PromptSubmissionMode::storage_key(),
                    PromptSubmissionMode::sync_to_cloud(),
                    &mut view.local_only_icon_tooltip_states.borrow_mut(),
                    app,
                ),
                (!is_any_ai_enabled).then(|| appearance.theme().disabled_ui_text_color()),
                &view.default_prompt_submission_mode_dropdown,
            ));
        }

        Flex::column().with_children(widget_children).finish()
    }
}

impl AIInputWidget {
    fn render_natural_language_detection_section(
        incorrect_autodetection_highlight_index: HighlightedHyperlink,
        autodetection_toggle: SwitchStateHandle,
        nld_in_terminal_toggle: SwitchStateHandle,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let is_toggleable = ai_settings.is_any_ai_enabled(app);
        let is_nld_enabled = *ai_settings.ai_autodetection_enabled_internal.value();

        let autodetection_denylist_input_field = appearance
            .ui_builder()
            .text_input(view.autodetection_denylist_editor.clone())
            .with_style(UiComponentStyles {
                width: Some(280.),
                padding: Some(Coords {
                    top: 4.,
                    bottom: 4.,
                    left: 6.,
                    right: 6.,
                }),
                background: Some(appearance.theme().surface_2().into()),
                ..Default::default()
            })
            .build()
            .finish();

        let mut section = Flex::column();

        if FeatureFlag::AgentView.is_enabled() {
            static AUTODETECTION_DESCRIPTION_FRAGMENTS: LazyLock<Vec<FormattedTextFragment>> =
                LazyLock::new(|| {
                    vec![
                        FormattedTextFragment::plain_text("Encountered an incorrect detection? "),
                        FormattedTextFragment::hyperlink(
                            "Let us know",
                            "https://warpdotdev.typeform.com/to/offrTIpq",
                        ),
                    ]
                });

            section.add_children([
                render_ai_setting_toggle::<NLDInTerminalEnabled>(
                    "Autodetect agent prompts in terminal input",
                    AISettingsPageAction::ToggleNLDInTerminal,
                    ai_settings.is_nld_in_terminal_enabled(app),
                    is_toggleable,
                    nld_in_terminal_toggle,
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
                render_ai_setting_toggle::<AIAutoDetectionEnabled>(
                    "Autodetect terminal commands in agent input",
                    AISettingsPageAction::ToggleAIInputAutoDetection,
                    is_nld_enabled,
                    is_toggleable,
                    autodetection_toggle,
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
                Container::new(
                    FormattedTextElement::new(
                        FormattedText::new([FormattedTextLine::Line(
                            (*AUTODETECTION_DESCRIPTION_FRAGMENTS).clone(),
                        )]),
                        CONTENT_FONT_SIZE,
                        appearance.ui_font_family(),
                        appearance.ui_font_family(),
                        styles::description_font_color(is_toggleable, app).into(),
                        incorrect_autodetection_highlight_index,
                    )
                    .with_hyperlink_font_color(appearance.theme().accent().into_solid())
                    .register_default_click_handlers(|url, ctx, _| {
                        ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
                    })
                    .finish(),
                )
                .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
                .finish(),
            ])
        } else {
            static NATURAL_LANGUAGE_DETECTION_DESCRIPTION_FRAGMENTS: LazyLock<
                Vec<FormattedTextFragment>,
            > = LazyLock::new(|| {
                vec![
                    FormattedTextFragment::plain_text(
                        "Enabling natural language detection will detect when natural language is written in the terminal input, and then automatically switch to Agent Mode for AI queries.",
                    ),
                    FormattedTextFragment::plain_text(
                        " Encountered an incorrect input detection? ",
                    ),
                    FormattedTextFragment::hyperlink(
                        "Let us know",
                        "https://warpdotdev.typeform.com/to/offrTIpq",
                    ),
                ]
            });

            section.add_children([
                render_ai_setting_toggle::<AIAutoDetectionEnabled>(
                    "Natural language detection",
                    AISettingsPageAction::ToggleAIInputAutoDetection,
                    is_nld_enabled,
                    is_toggleable,
                    autodetection_toggle,
                    &view.local_only_icon_tooltip_states,
                    app,
                ),
                Container::new(
                    FormattedTextElement::new(
                        FormattedText::new([FormattedTextLine::Line(
                            (*NATURAL_LANGUAGE_DETECTION_DESCRIPTION_FRAGMENTS).clone(),
                        )]),
                        CONTENT_FONT_SIZE,
                        appearance.ui_font_family(),
                        appearance.ui_font_family(),
                        styles::description_font_color(is_toggleable, app).into(),
                        incorrect_autodetection_highlight_index,
                    )
                    .with_hyperlink_font_color(appearance.theme().accent().into_solid())
                    .register_default_click_handlers(|url, ctx, _| {
                        ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
                    })
                    .finish(),
                )
                .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
                .finish(),
            ]);
        }

        section
            .with_child(render_ai_setting_label::<AICommandDenylist>(
                "Natural language denylist".to_owned(),
                is_toggleable,
                &view.local_only_icon_tooltip_states,
                app,
            ))
            .with_child(render_ai_setting_description(
                "Commands listed here will never trigger natural language detection.",
                is_toggleable,
                app,
            ))
            .with_child(
                Container::new(autodetection_denylist_input_field)
                    .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                    .finish(),
            )
            .finish()
    }
}

#[derive(Default)]
struct MCPServersWidget {
    manage_mcp_servers_button: MouseStateHandle,
    mcp_docs_link_index: HighlightedHyperlink,
    file_based_mcp_toggle: SwitchStateHandle,
    file_based_mcp_docs_link_index: HighlightedHyperlink,
}

impl MCPServersWidget {
    fn should_show_mcp() -> bool {
        FeatureFlag::McpServer.is_enabled() && ContextFlag::ShowMCPServers.is_enabled()
    }
}

impl SettingsWidget for MCPServersWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "oz agent mcp server servers model context protocol file-based file based project claude .mcp.json .claude/.mcp.json .codex config.toml .codex/config.toml"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        MCPServersWidget::should_show_mcp()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_any_ai_enabled = AISettings::as_ref(app).is_any_ai_enabled(app);
        let ai_settings = AISettings::as_ref(app);

        let header = build_sub_header(
            appearance,
            "MCP Servers",
            Some(styles::header_font_color(is_any_ai_enabled, app)),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();

        let mcp_description = vec![
            FormattedTextFragment::plain_text(
                "Add MCP servers to extend the Warp Agent's capabilities. \
            MCP servers expose data sources or tools to agents through a standardized interface, essentially acting like plugins. ",
            ),
            FormattedTextFragment::hyperlink(
                "Learn more",
                "https://docs.warp.dev/agent-platform/capabilities/mcp",
            ),
        ];

        let description = Container::new(
            FormattedTextElement::new(
                FormattedText::new([FormattedTextLine::Line(mcp_description)]),
                CONTENT_FONT_SIZE,
                appearance.ui_font_family(),
                appearance.ui_font_family(),
                styles::description_font_color(is_any_ai_enabled, app).into(),
                self.mcp_docs_link_index.clone(),
            )
            .with_hyperlink_font_color(appearance.theme().accent().into_solid())
            .register_default_click_handlers(|url, ctx, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
            })
            .finish(),
        )
        .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
        .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
        .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
        .finish();

        let file_based_mcp_toggle = if FeatureFlag::FileBasedMcp.is_enabled() {
            Some(
                Flex::column()
                    .with_child(render_ai_setting_toggle::<FileBasedMcpEnabled>(
                        "Auto-spawn servers from third-party agents",
                        AISettingsPageAction::ToggleFileBasedMcp,
                        *ai_settings.file_based_mcp_enabled,
                        is_any_ai_enabled,
                        self.file_based_mcp_toggle.clone(),
                        &view.local_only_icon_tooltip_states,
                        app,
                    ))
                    .with_child({
                        static FILE_BASED_MCP_DESCRIPTION_FRAGMENTS: LazyLock<
                            Vec<FormattedTextFragment>,
                        > = LazyLock::new(|| {
                            vec![
                                FormattedTextFragment::plain_text(
                                    "Automatically detect and spawn MCP servers from globally-scoped third-party AI agent configuration files (e.g. in your home directory). Servers detected inside a repository are never spawned automatically and must be enabled individually from the MCP settings page. ",
                                ),
                                FormattedTextFragment::hyperlink(
                                    "See supported providers.",
                                    "https://docs.warp.dev/agent-platform/capabilities/mcp#file-based-mcp-servers",
                                ),
                            ]
                        });
                        Container::new(
                            FormattedTextElement::new(
                                FormattedText::new([FormattedTextLine::Line(
                                    (*FILE_BASED_MCP_DESCRIPTION_FRAGMENTS).clone(),
                                )]),
                                CONTENT_FONT_SIZE,
                                appearance.ui_font_family(),
                                appearance.ui_font_family(),
                                styles::description_font_color(is_any_ai_enabled, app).into(),
                                self.file_based_mcp_docs_link_index.clone(),
                            )
                            .with_hyperlink_font_color(appearance.theme().accent().into_solid())
                            .register_default_click_handlers(|url, ctx, _| {
                                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
                            })
                            .finish(),
                        )
                        .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                        .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                        .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
                        .finish()
                    })
                    .finish(),
            )
        } else {
            None
        };

        let button = render_full_pane_width_ai_button(
            "Manage MCP servers",
            is_any_ai_enabled,
            self.manage_mcp_servers_button.clone(),
            AISettingsPageAction::OpenMCPServerCollection,
            appearance,
        );

        let mut column = Flex::column()
            .with_child(header)
            .with_child(description)
            .with_child(button);

        if let Some(toggle) = file_based_mcp_toggle {
            column = column.with_child(toggle);
        }
        column.finish()
    }
}

#[derive(Default)]
struct AIFactWidget {
    rules_toggle: SwitchStateHandle,
    rules_link_index: HighlightedHyperlink,
    manage_rules_button: MouseStateHandle,
    rule_suggestions_toggle: SwitchStateHandle,
    warp_drive_context_toggle: SwitchStateHandle,
}

impl AIFactWidget {
    fn render_rules_toggle(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &warpui::AppContext,
    ) -> Box<dyn Element> {
        let toggle = render_ai_setting_toggle::<MemoryEnabled>(
            "Rules",
            AISettingsPageAction::ToggleRules,
            *ai_settings.memory_enabled,
            ai_settings.is_any_ai_enabled(app),
            self.rules_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let rules_description = vec![
            FormattedTextFragment::plain_text(
                "Rules help the Warp Agent follow your conventions, whether for codebases or specific workflows. ",
            ),
            FormattedTextFragment::hyperlink(
                "Learn more",
                "https://docs.warp.dev/agent-platform/capabilities/rules",
            ),
        ];
        let description = Container::new(
            FormattedTextElement::new(
                FormattedText::new([FormattedTextLine::Line(rules_description)]),
                CONTENT_FONT_SIZE,
                appearance.ui_font_family(),
                appearance.ui_font_family(),
                styles::description_font_color(ai_settings.is_any_ai_enabled(app), app).into(),
                self.rules_link_index.clone(),
            )
            .with_hyperlink_font_color(appearance.theme().accent().into_solid())
            .register_default_click_handlers(|url, ctx, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
            })
            .finish(),
        )
        .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
        .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
        .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
        .finish();

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }

    fn render_rule_suggestions_toggle(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        app: &warpui::AppContext,
    ) -> Box<dyn Element> {
        let toggle = render_ai_setting_toggle::<RuleSuggestionsEnabled>(
            "Suggested Rules",
            AISettingsPageAction::ToggleRuleSuggestions,
            *ai_settings.rule_suggestions_enabled_internal,
            ai_settings.is_any_ai_enabled(app),
            self.rule_suggestions_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let description = render_ai_setting_description(
            "Let AI suggest rules to save based on your interactions.",
            ai_settings.is_any_ai_enabled(app),
            app,
        );

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }

    fn render_warp_drive_context_toggle(
        &self,
        view: &AISettingsPageView,
        ai_settings: &AISettings,
        app: &warpui::AppContext,
    ) -> Box<dyn Element> {
        let toggle = render_ai_setting_toggle::<WarpDriveContextEnabled>(
            "Warp Drive as agent context",
            AISettingsPageAction::ToggleWarpDriveContext,
            *ai_settings.warp_drive_context_enabled,
            ai_settings.is_any_ai_enabled(app),
            self.warp_drive_context_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let description = render_ai_setting_description(
            "The Warp Agent can leverage your Warp Drive Contents to tailor responses to your personal and team developer workflows and environments. This includes any Workflows, Notebooks, and Environment Variables.",
            ai_settings.is_any_ai_enabled(app),
            app,
        );

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }
}

impl SettingsWidget for AIFactWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "agent oz ai a.i. knowledge fact memory memories rules warp drive context workflows notebooks environment variables"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        FeatureFlag::AIRules.is_enabled()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let header = build_sub_header(
            appearance,
            "Knowledge",
            Some(styles::header_font_color(is_any_ai_enabled, app)),
        )
        .with_margin_bottom(HEADER_PADDING)
        .finish();

        let button = render_full_pane_width_ai_button(
            "Manage rules",
            is_any_ai_enabled,
            self.manage_rules_button.clone(),
            AISettingsPageAction::OpenAIFactCollection,
            appearance,
        );

        let mut column = Flex::column()
            .with_child(header)
            .with_child(self.render_rules_toggle(view, ai_settings, appearance, app));

        if FeatureFlag::SuggestedRules.is_enabled() {
            column.add_child(self.render_rule_suggestions_toggle(view, ai_settings, app));
        }

        column
            .with_child(button)
            .with_child(self.render_warp_drive_context_toggle(view, ai_settings, app))
            .finish()
    }
}

#[derive(Default)]
struct VoiceWidget {
    voice_input_toggle: SwitchStateHandle,
    wispr_highlight_index: HighlightedHyperlink,
}

impl VoiceWidget {
    fn render_voice_section(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &warpui::AppContext,
    ) -> Box<dyn warpui::Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_toggleable = ai_settings.is_any_ai_enabled(app);
        let mut column = Flex::column().with_child(render_ai_setting_toggle::<VoiceInputEnabled>(
            "Voice Input",
            AISettingsPageAction::ToggleVoiceInput,
            *ai_settings.voice_input_enabled_internal,
            is_toggleable,
            self.voice_input_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        ));

        let voice_input_description_text_fragments = vec![
            FormattedTextFragment::plain_text(
                "Voice input allows you to control Warp by speaking directly to your terminal (powered by ",
            ),
            FormattedTextFragment::hyperlink("Wispr Flow", WISPR_FLOW_URL),
            FormattedTextFragment::plain_text(")."),
        ];

        let voice_input_description = FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(
                voice_input_description_text_fragments,
            )]),
            appearance.ui_font_size(),
            appearance.ui_font_family(),
            appearance.ui_font_family(),
            styles::description_font_color(is_toggleable, app).into(),
            self.wispr_highlight_index.clone(),
        )
        .with_hyperlink_font_color(appearance.theme().accent().into_solid())
        .register_default_click_handlers(|url, ctx, _| {
            ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
        });

        column.add_child(
            Container::new(voice_input_description.finish())
                .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
                .finish(),
        );

        if ai_settings.is_voice_input_enabled(app) {
            column.add_child(render_dropdown_item(
                appearance,
                "Key for Activating Voice Input",
                Some("Press and hold to activate."),
                None,
                LocalOnlyIconState::for_setting(
                    VoiceInputToggleKey::storage_key(),
                    VoiceInputToggleKey::sync_to_cloud(),
                    &mut view.local_only_icon_tooltip_states.borrow_mut(),
                    app,
                ),
                None,
                &view.voice_input_toggle_key_dropdown,
            ));
        }

        column.finish()
    }
}

impl SettingsWidget for VoiceWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "voice agent oz ai a.i. speech input natural language talk english"
    }

    fn should_render(&self, app: &AppContext) -> bool {
        cfg!(feature = "voice_input") && UserWorkspaces::as_ref(app).is_voice_enabled()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "Voice",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(self.render_voice_section(view, appearance, app))
            .finish()
    }
}
#[derive(Default)]
struct OtherAIWidget {
    show_oz_updates_in_zero_state_toggle: SwitchStateHandle,
    use_agent_footer_toggle: SwitchStateHandle,
    show_conversation_history_toggle: SwitchStateHandle,
}

impl OtherAIWidget {
    fn create_thinking_display_mode_dropdown(
        ctx: &mut ViewContext<AISettingsPageView>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        let items: Vec<DropdownItem<AISettingsPageAction>> = ThinkingDisplayMode::iter()
            .map(|mode| {
                DropdownItem::new(
                    mode.display_name(),
                    AISettingsPageAction::SetThinkingDisplayMode(mode),
                )
            })
            .collect();

        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_max_height(AI_SETTINGS_DROPDOWN_MAX_HEIGHT, ctx);
            dropdown.add_items(items, ctx);
            dropdown
        })
    }

    fn create_default_prompt_submission_mode_dropdown(
        ctx: &mut ViewContext<AISettingsPageView>,
    ) -> ViewHandle<Dropdown<AISettingsPageAction>> {
        let items: Vec<DropdownItem<AISettingsPageAction>> = PromptSubmissionMode::iter()
            .map(|mode| {
                DropdownItem::new(
                    mode.display_name(),
                    AISettingsPageAction::SetPromptSubmissionMode(mode),
                )
            })
            .collect();

        ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(AI_SETTINGS_DROPDOWN_WIDTH);
            dropdown.set_menu_width(AI_SETTINGS_DROPDOWN_WIDTH, ctx);
            dropdown.set_menu_max_height(AI_SETTINGS_DROPDOWN_MAX_HEIGHT, ctx);
            dropdown.add_items(items, ctx);
            dropdown
        })
    }
}

impl SettingsWidget for OtherAIWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "other oz updates zero state empty changelog new conversation agent what's new use agent footer toolbar layout chip chips rearrange re-arrange thinking expanded reasoning collapse never show hide conversation history"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let is_toggleable = is_any_ai_enabled;

        let mut column = Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "Other",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            );

        if FeatureFlag::AgentView.is_enabled() {
            let mut agent_view_column = Flex::column()
                .with_child(render_ai_setting_toggle::<ShouldShowOzUpdatesInZeroState>(
                    "Show Oz changelog in new conversation view",
                    AISettingsPageAction::ToggleShowOzUpdatesInZeroState,
                    *ai_settings.should_show_oz_updates_in_zero_state,
                    is_toggleable,
                    self.show_oz_updates_in_zero_state_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ))
                .with_child(render_ai_setting_toggle::<ShouldRenderUseAgentToolbarForUserCommands>(
                    "Show \"Use Agent\" footer",
                    AISettingsPageAction::ToggleUseAgentToolbar,
                    *ai_settings.should_render_use_agent_footer_for_user_commands,
                    is_toggleable,
                    self.use_agent_footer_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ))
                .with_child(render_ai_setting_description(
                    "Shows hint to use the \"Full Terminal Use\"-enabled agent in long running commands.",
                    is_toggleable,
                    app,
                ));

            if is_toggleable && FeatureFlag::AgentToolbarEditor.is_enabled() {
                agent_view_column.add_child(render_toolbar_layout_editor(
                    &view.agent_toolbar_inline_editor,
                    appearance,
                ));
            }

            column.add_child(agent_view_column.finish());
        }

        column.add_child(render_ai_setting_toggle::<ShowConversationHistory>(
            "Show conversation history in tools panel",
            AISettingsPageAction::ToggleShowConversationHistory,
            *ai_settings.show_conversation_history,
            is_toggleable,
            self.show_conversation_history_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        ));

        column.add_child(render_dropdown_item(
            appearance,
            "Agent thinking display",
            Some("Controls how reasoning/thinking traces are displayed."),
            None,
            LocalOnlyIconState::for_setting(
                ThinkingDisplayMode::storage_key(),
                ThinkingDisplayMode::sync_to_cloud(),
                &mut view.local_only_icon_tooltip_states.borrow_mut(),
                app,
            ),
            (!is_any_ai_enabled).then(|| appearance.theme().disabled_ui_text_color()),
            &view.thinking_display_mode_dropdown,
        ));

        // TODO: OpenConversationLayoutPreference should not depend on local_fs, but it lives under the external editor settings
        // which does require local_fs. It was a mistake to put it there, but now we keep it there for backward compatibility.
        #[cfg(feature = "local_fs")]
        if FeatureFlag::OpenWarpNewSettingsModes.is_enabled() {
            use crate::util::file::external_editor::settings::OpenConversationLayoutPreference;

            column.add_child(render_dropdown_item(
                appearance,
                "Preferred layout when opening existing agent conversations",
                None,
                None,
                LocalOnlyIconState::for_setting(
                    OpenConversationLayoutPreference::storage_key(),
                    OpenConversationLayoutPreference::sync_to_cloud(),
                    &mut view.local_only_icon_tooltip_states.borrow_mut(),
                    app,
                ),
                (!is_any_ai_enabled).then(|| appearance.theme().disabled_ui_text_color()),
                &view.conversation_layout_dropdown,
            ));
        }

        column.finish()
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn cli_agent_settings_widget_id() -> &'static str {
    CLIAgentWidget::static_widget_id()
}

#[derive(Default)]
struct CLIAgentWidget {
    cli_agent_footer_toggle: SwitchStateHandle,
    auto_toggle_rich_input_toggle: SwitchStateHandle,
    auto_toggle_rich_input_info_tooltip: MouseStateHandle,
    auto_open_rich_input_on_cli_agent_start_toggle: SwitchStateHandle,
    auto_dismiss_rich_input_toggle: SwitchStateHandle,
}

impl SettingsWidget for CLIAgentWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "third party cli coding agent claude codex gemini toolbar footer layout chip chips rearrange re-arrange bar command regex auto show rich input dismiss"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);

        // The Coding Agents section is always enabled, independent of the
        // global AI toggle, because these settings control third-party coding
        // agents (Claude Code, Codex, Gemini CLI) rather than Warp's own AI.
        let cli_agent_footer_toggle = render_ai_setting_toggle::<ShouldRenderCLIAgentToolbar>(
            "Show coding agent toolbar",
            AISettingsPageAction::ToggleCLIAgentToolbar,
            *ai_settings.should_render_cli_agent_footer,
            true,
            self.cli_agent_footer_toggle.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let description_fragments = vec![
            FormattedTextFragment::plain_text(
                "Show a toolbar with quick actions when running coding agents like ",
            ),
            FormattedTextFragment::inline_code("claude"),
            FormattedTextFragment::plain_text(", "),
            FormattedTextFragment::inline_code("codex"),
            FormattedTextFragment::plain_text(", or "),
            FormattedTextFragment::inline_code("gemini"),
            FormattedTextFragment::plain_text("."),
        ];

        let description = FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(description_fragments)]),
            appearance.ui_font_size(),
            appearance.ui_font_family(),
            appearance.monospace_font_family(),
            styles::description_font_color(true, app).into(),
            HighlightedHyperlink::default(),
        );

        let is_footer_enabled = *ai_settings.should_render_cli_agent_footer;

        let mut column = Flex::column()
            .with_child(
                build_sub_header(
                    appearance,
                    "Third party CLI agents",
                    Some(styles::header_font_color(true, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(cli_agent_footer_toggle)
            .with_child(
                Container::new(description.finish())
                    .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
                    .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                    .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
                    .finish(),
            );

        if is_footer_enabled {
            use super::settings_page::AdditionalInfo;
            use crate::settings::{
                AutoDismissRichInputAfterSubmit, AutoOpenRichInputOnCLIAgentStart,
                AutoToggleRichInput,
            };

            if FeatureFlag::CLIAgentRichInput.is_enabled() {
                // Setting 1: Auto show/hide rich input based on agent status
                let auto_show_toggle_label = render_body_item_label::<AISettingsPageAction>(
                    "Auto show/hide Rich Input based on agent status".into(),
                    Some(styles::header_font_color(true, app)),
                    Some(AdditionalInfo {
                        mouse_state: self.auto_toggle_rich_input_info_tooltip.clone(),
                        on_click_action: None,
                        secondary_text: None,
                        tooltip_override_text: Some(
                            "Requires the Warp plugin for your coding agent".to_owned(),
                        ),
                    }),
                    LocalOnlyIconState::for_setting(
                        AutoToggleRichInput::storage_key(),
                        AutoToggleRichInput::sync_to_cloud(),
                        &mut view.local_only_icon_tooltip_states.borrow_mut(),
                        app,
                    ),
                    ToggleState::Enabled,
                    appearance,
                );
                column.add_child(build_toggle_element(
                    auto_show_toggle_label,
                    render_ai_feature_switch(
                        self.auto_toggle_rich_input_toggle.clone(),
                        *ai_settings.auto_toggle_rich_input,
                        true,
                        AISettingsPageAction::ToggleAutoToggleRichInput,
                        app,
                    ),
                    appearance,
                    None,
                ));

                column.add_child(
                    render_ai_setting_toggle::<AutoOpenRichInputOnCLIAgentStart>(
                        "Auto open Rich Input when a coding agent session starts",
                        AISettingsPageAction::ToggleAutoOpenRichInputOnCLIAgentStart,
                        *ai_settings.auto_open_rich_input_on_cli_agent_start,
                        true,
                        self.auto_open_rich_input_on_cli_agent_start_toggle.clone(),
                        &view.local_only_icon_tooltip_states,
                        app,
                    ),
                );

                // Setting 2: Auto dismiss rich input after prompt submission
                column.add_child(render_ai_setting_toggle::<AutoDismissRichInputAfterSubmit>(
                    "Auto dismiss Rich Input after prompt submission",
                    AISettingsPageAction::ToggleAutoDismissRichInputAfterSubmit,
                    *ai_settings.auto_dismiss_rich_input_after_submit,
                    true,
                    self.auto_dismiss_rich_input_toggle.clone(),
                    &view.local_only_icon_tooltip_states,
                    app,
                ));
            }

            let command_list = {
                let mut list_column = Flex::column();

                list_column.add_child(
                    appearance
                        .ui_builder()
                        .span("Commands that enable the toolbar".to_string())
                        .with_style(UiComponentStyles {
                            font_size: Some(CONTENT_FONT_SIZE),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                );

                list_column
                    .add_child(ChildView::new(&view.cli_agent_footer_command_editor).finish());

                let background = appearance.theme().surface_1();
                let font_color = appearance.theme().foreground();
                let items: Vec<_> = ai_settings
                    .cli_agent_footer_enabled_commands
                    .value()
                    .keys()
                    .cloned()
                    .collect();
                let len = items.len();
                for (rev_i, pattern) in items.iter().rev().enumerate() {
                    let original_i = len - 1 - rev_i;
                    let remove_action =
                        AISettingsPageAction::RemoveCLIAgentToolbarEnabledCommand(pattern.clone());
                    let mouse_state = view
                        .cli_agent_footer_command_mouse_state_handles
                        .get(original_i)
                        .cloned()
                        .unwrap_or_default();

                    let remove_button = appearance
                        .ui_builder()
                        .close_button(16., mouse_state)
                        .build()
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(remove_action.clone());
                        })
                        .finish();

                    let label = appearance
                        .ui_builder()
                        .wrappable_text(pattern.clone(), true)
                        .with_style(UiComponentStyles {
                            font_color: Some(font_color.into_solid()),
                            font_family_id: Some(appearance.monospace_font_family()),
                            font_size: Some(appearance.ui_font_size()),
                            ..Default::default()
                        })
                        .build()
                        .finish();

                    let mut right_side =
                        Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
                    if let Some(dropdown_handle) = view
                        .cli_agent_footer_command_agent_dropdowns
                        .get(original_i)
                    {
                        right_side.add_child(
                            Container::new(ChildView::new(dropdown_handle).finish())
                                .with_margin_right(8.)
                                .finish(),
                        );
                    }
                    right_side.add_child(remove_button);

                    let row = Container::new(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .with_children([
                                Shrinkable::new(1., label).finish(),
                                right_side.finish(),
                            ])
                            .finish(),
                    )
                    .with_background(background)
                    .with_horizontal_padding(8.)
                    .with_vertical_padding(4.)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                    .with_margin_bottom(4.)
                    .finish();

                    list_column.add_child(row);
                }

                list_column.finish()
            };
            let command_list_description = appearance
                .ui_builder()
                .paragraph(
                    "Add regex patterns to show the coding agent toolbar for matching commands.",
                )
                .with_style(UiComponentStyles {
                    font_size: Some(appearance.ui_font_size()),
                    font_color: Some(styles::description_font_color(true, app).into()),
                    margin: Some(
                        Coords::default()
                            .top(4.)
                            .bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                            .right(styles::TOGGLE_WIDTH_MARGIN),
                    ),
                    ..Default::default()
                })
                .build()
                .finish();

            column.add_child(command_list);
            column.add_child(command_list_description);

            if FeatureFlag::AgentToolbarEditor.is_enabled() {
                column.add_child(render_toolbar_layout_editor(
                    &view.cli_agent_toolbar_inline_editor,
                    appearance,
                ));
            }
        }

        column.finish()
    }
}

#[derive(Default)]
struct CLIAgentApiProfilesWidget;

impl CLIAgentApiProfilesWidget {
    fn render_input(
        appearance: &Appearance,
        label: &'static str,
        editor: ViewHandle<EditorView>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let input = appearance
            .ui_builder()
            .text_input(editor)
            .with_style(UiComponentStyles {
                padding: Some(Coords::default().top(6.).bottom(6.).left(10.).right(10.)),
                background: Some(appearance.theme().surface_2().into()),
                ..Default::default()
            })
            .build()
            .finish();

        Flex::column()
            .with_spacing(4.)
            .with_child(
                Text::new_inline(label, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                    .with_color(styles::header_font_color(true, app).into())
                    .finish(),
            )
            .with_child(input)
            .finish()
    }

    fn render_multiline_input(
        appearance: &Appearance,
        label: &'static str,
        editor: ViewHandle<EditorView>,
        height: f32,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let editor = Container::new(
            ConstrainedBox::new(ChildView::new(&editor).finish())
                .with_height(height)
                .finish(),
        )
        .with_background(appearance.theme().surface_2())
        .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_horizontal_padding(8.)
        .with_vertical_padding(6.)
        .finish();

        Flex::column()
            .with_spacing(6.)
            .with_child(
                Text::new_inline(label, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                    .with_color(styles::header_font_color(true, app).into())
                    .finish(),
            )
            .with_child(editor)
            .finish()
    }

    fn render_section(
        appearance: &Appearance,
        title: &'static str,
        child: Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        Container::new(
            Flex::column()
                .with_spacing(6.)
                .with_child(
                    Text::new_inline(title, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                        .with_color(styles::header_font_color(true, app).into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                )
                .with_child(child)
                .finish(),
        )
        .with_vertical_padding(4.)
        .finish()
    }

    fn render_hint_text(
        appearance: &Appearance,
        text: impl Into<String>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .paragraph(text.into())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_color: Some(styles::description_font_color(true, app).into()),
                ..Default::default()
            })
            .build()
            .finish()
    }

    fn render_icon_action_button(
        appearance: &Appearance,
        icon: Icon,
        action: AISettingsPageAction,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Hoverable::new(MouseStateHandle::default(), move |state| {
            let icon_fill = if state.is_hovered() {
                theme.active_ui_text_color().into()
            } else {
                styles::description_font_color(true, app).into()
            };
            Container::new(
                Align::new(
                    ConstrainedBox::new(icon.to_warpui_icon(icon_fill).finish())
                        .with_width(14.)
                        .with_height(14.)
                        .finish(),
                )
                .finish(),
            )
            .with_horizontal_padding(5.)
            .with_vertical_padding(5.)
            .with_background(if state.is_hovered() {
                internal_colors::fg_overlay_1(theme)
            } else {
                theme.surface_1()
            })
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_model_row(
        appearance: &Appearance,
        model: String,
        trailing: Box<dyn Element>,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_children([
                    Shrinkable::new(
                        1.,
                        Text::new_inline(
                            model,
                            appearance.monospace_font_family(),
                            CONTENT_FONT_SIZE,
                        )
                        .with_color(theme.active_ui_text_color().into())
                        .finish(),
                    )
                    .finish(),
                    trailing,
                ])
                .finish(),
        )
        .with_horizontal_padding(8.)
        .with_vertical_padding(5.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
    }

    fn render_current_models(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let models = AISettingsPageView::parse_cli_agent_api_model_catalog(
            &view
                .cli_agent_api_profile_model_catalog_editor
                .as_ref(app)
                .buffer_text(app),
        );
        let preferred_model = view
            .cli_agent_api_profile_model_editor
            .as_ref(app)
            .buffer_text(app);
        let mut list = Flex::column().with_spacing(6.);

        if models.is_empty() {
            list.add_child(Self::render_hint_text(appearance, "还没有添加模型。", app));
        }

        for model in models {
            let is_preferred = preferred_model.trim().eq_ignore_ascii_case(&model);
            let model_for_use = model.clone();
            let model_for_remove = model.clone();
            let status_or_use = Self::render_icon_action_button(
                appearance,
                Icon::Link,
                AISettingsPageAction::UseCLIAgentApiProfileModel(model_for_use),
                app,
            );
            let remove_button = Self::render_icon_action_button(
                appearance,
                Icon::Trash,
                AISettingsPageAction::RemoveCLIAgentApiProfileModel(model_for_remove),
                app,
            );
            let trailing = Flex::row()
                .with_spacing(6.)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(if is_preferred {
                    Text::new_inline("默认", appearance.ui_font_family(), CONTENT_FONT_SIZE)
                        .with_color(theme.accent().into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish()
                } else {
                    Empty::new().finish()
                })
                .with_child(status_or_use)
                .with_child(remove_button)
                .finish();
            list.add_child(Self::render_model_row(appearance, model, trailing, app));
        }

        let add_model_button = view
            .cli_agent_api_profile_toggle_add_model_button
            .as_ref(app)
            .render(app);

        let mut column = Flex::column()
            .with_spacing(8.)
            .with_child(list.finish())
            .with_child(add_model_button);

        if view.cli_agent_api_profile_add_model_open {
            column.add_child(self.render_add_model_panel(view, appearance, app));
        }

        column.finish()
    }

    fn render_add_model_panel(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let status = if view.cli_agent_api_profile_fetching_models {
            Some("正在获取模型...")
        } else if view.cli_agent_api_profile_fetch_models_error.is_some() {
            Some("获取模型失败，请检查接口地址和 API Key。")
        } else if view.cli_agent_api_profile_fetched_models.is_empty() {
            Some("点击获取模型后，可从下拉列表选择并填入。")
        } else {
            None
        };

        let mut content = Flex::column().with_spacing(8.).with_child(
            Flex::row()
                .with_spacing(8.)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_children([
                    view.cli_agent_api_profile_fetch_models_button
                        .as_ref(app)
                        .render(app),
                    Shrinkable::new(
                        1.,
                        status
                            .map(|status| Self::render_hint_text(appearance, status, app))
                            .unwrap_or_else(|| {
                                Self::render_hint_text(
                                    appearance,
                                    "选择模型后会自动填入，也可以手动输入。",
                                    app,
                                )
                            }),
                    )
                    .finish(),
                ])
                .finish(),
        );

        if !view.cli_agent_api_profile_fetched_models.is_empty() {
            content.add_child(
                Flex::row()
                    .with_spacing(8.)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_children([
                        Expanded::new(
                            1.,
                            ChildView::new(&view.cli_agent_api_profile_model_picker_dropdown)
                                .finish(),
                        )
                        .finish(),
                        view.cli_agent_api_profile_add_selected_model_button
                            .as_ref(app)
                            .render(app),
                    ])
                    .finish(),
            );
        }

        content.add_child(
            Flex::row()
                .with_spacing(8.)
                .with_cross_axis_alignment(CrossAxisAlignment::End)
                .with_children([
                    Expanded::new(
                        1.,
                        Self::render_input(
                            appearance,
                            "手动输入模型",
                            view.cli_agent_api_profile_model_editor.clone(),
                            app,
                        ),
                    )
                    .finish(),
                    view.cli_agent_api_profile_add_manual_model_button
                        .as_ref(app)
                        .render(app),
                ])
                .finish(),
        );

        Container::new(content.finish())
            .with_horizontal_padding(10.)
            .with_vertical_padding(10.)
            .with_background(theme.surface_2())
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish()
    }

    fn render_model_mapping_section(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let chevron = if view.cli_agent_api_profile_model_mapping_open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let header = Hoverable::new(MouseStateHandle::default(), move |state| {
            let color = if state.is_hovered() {
                theme.active_ui_text_color()
            } else {
                styles::description_font_color(true, app)
            };
            Flex::row()
                .with_spacing(4.)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(
                        "Claude 模型映射",
                        appearance.ui_font_family(),
                        CONTENT_FONT_SIZE,
                    )
                    .with_color(color.into())
                    .finish(),
                )
                .with_child(
                    ConstrainedBox::new(chevron.to_warpui_icon(color.into()).finish())
                        .with_width(12.)
                        .with_height(12.)
                        .finish(),
                )
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(AISettingsPageAction::ToggleCLIAgentApiProfileModelMapping);
        })
        .finish();

        let mut column = Flex::column().with_spacing(8.).with_child(header);
        if view.cli_agent_api_profile_model_mapping_open {
            column.add_child(Self::render_multiline_input(
                appearance,
                "映射配置",
                view.cli_agent_api_profile_model_mappings_editor.clone(),
                72.,
                app,
            ));
        }

        Flex::column()
            .with_spacing(8.)
            .with_child(column.finish())
            .finish()
    }

    fn render_profile_form(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        Flex::column()
            .with_spacing(12.)
            .with_child(Self::render_input(
                appearance,
                "供应商名称",
                view.cli_agent_api_profile_name_editor.clone(),
                app,
            ))
            .with_child(Self::render_input(
                appearance,
                "Anthropic 接口地址",
                view.cli_agent_api_profile_base_url_editor.clone(),
                app,
            ))
            .with_child(Self::render_input(
                appearance,
                "OpenAI 接口地址",
                view.cli_agent_api_profile_openai_base_url_editor.clone(),
                app,
            ))
            .with_child(Self::render_input(
                appearance,
                "API Key",
                view.cli_agent_api_profile_api_key_editor.clone(),
                app,
            ))
            .with_child(
                Flex::row()
                    .with_spacing(8.)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(ChildView::new(&view.cli_agent_api_profile_agent_dropdown).finish())
                    .with_child(
                        ChildView::new(&view.cli_agent_api_profile_environment_dropdown).finish(),
                    )
                    .finish(),
            )
            .with_child(Self::render_section(
                appearance,
                "模型列表",
                self.render_current_models(view, appearance, app),
                app,
            ))
            .with_child(self.render_model_mapping_section(view, appearance, app))
            .finish()
    }

    fn render_status_badge(
        appearance: &Appearance,
        label: &str,
        active: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let background = if active {
            internal_colors::fg_overlay_1(theme)
        } else {
            theme.surface_2()
        };
        let color = if active {
            theme.accent()
        } else {
            styles::description_font_color(true, app).into()
        };

        Container::new(
            Text::new_inline(
                label.to_owned(),
                appearance.ui_font_family(),
                CONTENT_FONT_SIZE,
            )
            .with_color(color.into())
            .with_style(Properties::default().weight(Weight::Semibold))
            .finish(),
        )
        .with_horizontal_padding(8.)
        .with_vertical_padding(3.)
        .with_background(background)
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(999.)))
        .finish()
    }

    fn render_provider_sidebar(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let store = AISettings::as_ref(app).cli_agent_api_profiles();
        let selected_profile_id = view.cli_agent_api_profile_editing_profile_id.as_deref();
        let description_color = styles::description_font_color(true, app);

        let mut list = Flex::column().with_spacing(6.);
        if store.profiles.is_empty() {
            list.add_child(
                appearance
                    .ui_builder()
                    .paragraph("还没有自定义供应商。")
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        font_color: Some(description_color.into()),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );
        }

        for (index, profile) in store.profiles.iter().enumerate() {
            let agent = profile.agent();
            let environment_id = profile.environment_id.clone();
            let active_profile_id = AISettings::as_ref(app)
                .active_cli_agent_api_profile(agent, &environment_id)
                .map(|profile| profile.id.clone());
            let is_active = active_profile_id.as_deref() == Some(profile.id.as_str());
            let is_selected = selected_profile_id == Some(profile.id.as_str());
            let row_mouse_state = view
                .cli_agent_api_profile_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();
            let profile_id = profile.id.clone();
            let profile_name = profile.name.clone();
            let model = profile.preferred_model();
            let details = if model.trim().is_empty() {
                format!(
                    "{} / {}",
                    agent.display_name(),
                    AISettingsPageView::cli_agent_api_environment_label(&environment_id, app)
                )
            } else {
                format!(
                    "{} / {} / {}",
                    agent.display_name(),
                    AISettingsPageView::cli_agent_api_environment_label(&environment_id, app),
                    model
                )
            };
            let status_fill = if profile.enabled && is_active {
                theme.accent()
            } else if profile.enabled {
                theme.ansi_fg_green().into()
            } else {
                theme.nonactive_ui_detail().into()
            };

            let row = Hoverable::new(row_mouse_state, move |state| {
                let background = if is_selected {
                    theme.surface_overlay_1()
                } else if state.is_hovered() {
                    internal_colors::fg_overlay_1(theme)
                } else {
                    theme.surface_1()
                };
                let icon = ConstrainedBox::new(
                    Icon::Globe
                        .to_warpui_icon(styles::description_font_color(true, app).into())
                        .finish(),
                )
                .with_width(14.)
                .with_height(14.)
                .finish();
                let title = Text::new_inline(
                    profile_name.clone(),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(theme.active_ui_text_color().into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish();
                let details = Text::new_inline(
                    details.clone(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(styles::description_font_color(true, app).into())
                .finish();
                let dot =
                    ConstrainedBox::new(Icon::CircleFilled.to_warpui_icon(status_fill).finish())
                        .with_width(8.)
                        .with_height(8.)
                        .finish();

                Container::new(
                    Flex::row()
                        .with_spacing(8.)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_children([
                            icon,
                            Shrinkable::new(
                                1.,
                                Flex::column()
                                    .with_spacing(2.)
                                    .with_child(title)
                                    .with_child(details)
                                    .finish(),
                            )
                            .finish(),
                            dot,
                        ])
                        .finish(),
                )
                .with_horizontal_padding(10.)
                .with_vertical_padding(8.)
                .with_background(background)
                .with_border(Border::all(1.).with_border_fill(theme.outline()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .finish()
            })
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(AISettingsPageAction::EditCLIAgentApiProfile(
                    profile_id.clone(),
                ));
            })
            .finish();
            list.add_child(row);
        }

        let header = Text::new_inline(
            "自定义供应商",
            appearance.ui_font_family(),
            CONTENT_FONT_SIZE,
        )
        .with_color(styles::header_font_color(true, app).into())
        .with_style(Properties::default().weight(Weight::Semibold))
        .finish();
        let add_button = view
            .cli_agent_api_profile_open_add_button
            .as_ref(app)
            .render(app);

        Container::new(
            Flex::column()
                .with_spacing(10.)
                .with_child(header)
                .with_child(list.finish())
                .with_child(add_button)
                .finish(),
        )
        .with_uniform_padding(12.)
        .with_background(theme.surface_1())
        .with_border(Border::right(1.).with_border_fill(theme.outline()))
        .finish()
    }

    fn render_provider_detail_actions(
        &self,
        view: &AISettingsPageView,
        selected_profile: Option<&CLIAgentApiProfile>,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let description_color = styles::description_font_color(true, app);
        let mut actions = Flex::row()
            .with_spacing(8.)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                view.cli_agent_api_profile_add_button
                    .as_ref(app)
                    .render(app),
            );

        if let Some(profile) = selected_profile {
            let toggle_profile_id = profile.id.clone();
            let toggle_enabled = !profile.enabled;
            let toggle_button = appearance
                .ui_builder()
                .button(ButtonVariant::Text, MouseStateHandle::default())
                .with_text_label(if profile.enabled {
                    "禁用".to_owned()
                } else {
                    "启用".to_owned()
                })
                .with_style(UiComponentStyles {
                    font_size: Some(CONTENT_FONT_SIZE),
                    font_weight: Some(Weight::Semibold),
                    font_color: Some(if profile.enabled {
                        description_color.into()
                    } else {
                        theme.accent().into()
                    }),
                    padding: Some(Coords::default().top(4.).bottom(4.).left(6.).right(6.)),
                    ..Default::default()
                })
                .build()
                .with_cursor(Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AISettingsPageAction::SetCLIAgentApiProfileEnabled {
                        profile_id: toggle_profile_id.clone(),
                        enabled: toggle_enabled,
                    });
                })
                .finish();
            actions.add_child(toggle_button);

            let remove_profile_id = profile.id.clone();
            let remove_button = Self::render_icon_action_button(
                appearance,
                Icon::Trash,
                AISettingsPageAction::RemoveCLIAgentApiProfile(remove_profile_id),
                app,
            );
            actions.add_child(remove_button);
        }

        if let Some(message) = view.cli_agent_api_profile_save_feedback.as_ref() {
            let color = if view.cli_agent_api_profile_save_feedback_is_error {
                theme.ansi_fg_red().into()
            } else {
                theme.ansi_fg_green().into()
            };
            actions.add_child(
                Text::new_inline(
                    message.clone(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_color(color)
                .finish(),
            );
        }

        actions.finish()
    }

    fn render_provider_detail(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let store = AISettings::as_ref(app).cli_agent_api_profiles();
        let selected_profile = view
            .cli_agent_api_profile_editing_profile_id
            .as_deref()
            .and_then(|profile_id| {
                store
                    .profiles
                    .iter()
                    .find(|profile| profile.id == profile_id)
            });
        let draft_title = view
            .cli_agent_api_profile_name_editor
            .as_ref(app)
            .buffer_text(app);
        let title = if !draft_title.trim().is_empty() {
            draft_title.trim().to_owned()
        } else {
            selected_profile
                .map(|profile| profile.name.clone())
                .unwrap_or_else(|| "新供应商".to_owned())
        };
        let status_label = selected_profile
            .map(|profile| {
                if profile.enabled {
                    "已启用"
                } else {
                    "已禁用"
                }
            })
            .unwrap_or("草稿");
        let status_active = selected_profile
            .map(|profile| profile.enabled)
            .unwrap_or(false);

        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(
                Flex::row()
                    .with_spacing(8.)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new_inline(title, appearance.ui_font_family(), 18.)
                            .with_color(theme.active_ui_text_color().into())
                            .with_style(Properties::default().weight(Weight::Semibold))
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            Icon::Pencil
                                .to_warpui_icon(styles::description_font_color(true, app).into())
                                .finish(),
                        )
                        .with_width(14.)
                        .with_height(14.)
                        .finish(),
                    )
                    .with_child(Self::render_status_badge(
                        appearance,
                        status_label,
                        status_active,
                        app,
                    ))
                    .finish(),
            )
            .with_child(self.render_provider_detail_actions(
                view,
                selected_profile,
                appearance,
                app,
            ))
            .finish();

        Container::new(
            Flex::column()
                .with_spacing(14.)
                .with_child(header)
                .with_child(self.render_profile_form(view, appearance, app))
                .finish(),
        )
        .with_uniform_padding(18.)
        .with_background(theme.surface_1())
        .finish()
    }

    fn render_provider_manager(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_children([
                    ConstrainedBox::new(self.render_provider_sidebar(view, appearance, app))
                        .with_width(240.)
                        .finish(),
                    Expanded::new(1., self.render_provider_detail(view, appearance, app)).finish(),
                ])
                .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .finish()
    }

    #[allow(dead_code)]
    fn render_json_tools(
        &self,
        _view: &AISettingsPageView,
        _appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        Empty::new().finish()
    }

    #[allow(dead_code)]
    fn render_usage_summary(
        &self,
        _view: &AISettingsPageView,
        _appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        Empty::new().finish()
    }

    #[allow(dead_code)]
    fn render_profiles(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let store = AISettings::as_ref(app).cli_agent_api_profiles();
        if store.profiles.is_empty() {
            return appearance
                .ui_builder()
                .paragraph("No agent API profiles configured yet.")
                .with_style(UiComponentStyles {
                    font_size: Some(appearance.ui_font_size()),
                    font_color: Some(styles::description_font_color(true, app).into()),
                    margin: Some(Coords::default().top(4.)),
                    ..Default::default()
                })
                .build()
                .finish();
        }

        let mut list = Flex::column().with_spacing(8.);
        for (index, profile) in store.profiles.iter().enumerate() {
            let agent = profile.agent();
            let environment_id = profile.environment_id.clone();
            let active_profile_id = AISettings::as_ref(app)
                .active_cli_agent_api_profile(agent, &environment_id)
                .map(|profile| profile.id.clone());
            let is_active = active_profile_id.as_deref() == Some(profile.id.as_str());
            let row_mouse_state = view
                .cli_agent_api_profile_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();
            let remove_mouse_state = view
                .cli_agent_api_profile_remove_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();
            let edit_mouse_state = view
                .cli_agent_api_profile_edit_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();
            let check_mouse_state = view
                .cli_agent_api_profile_check_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();
            let toggle_mouse_state = view
                .cli_agent_api_profile_toggle_mouse_state_handles
                .get(index)
                .cloned()
                .unwrap_or_default();

            let profile_id = profile.id.clone();
            let profile_id_for_row = profile_id.clone();
            let remove_profile_id = profile_id.clone();
            let edit_profile_id = profile_id.clone();
            let check_profile_id = profile_id.clone();
            let toggle_profile_id = profile_id.clone();
            let profile_name = profile.name.clone();
            let environment_label =
                AISettingsPageView::cli_agent_api_environment_label(&environment_id, app);
            let base_url = if profile.base_url.is_empty() {
                "Default endpoint".to_owned()
            } else {
                profile.base_url.clone()
            };
            let model = if profile.model.is_empty() {
                let preferred_model = profile.preferred_model();
                if preferred_model.trim().is_empty() {
                    "Default model".to_owned()
                } else {
                    preferred_model
                }
            } else {
                profile.model.clone()
            };
            let key_label = AISettingsPageView::masked_api_key(&profile.api_key);
            let agent_name = agent.display_name().to_owned();
            let priority_label = format!("priority {}", profile.priority);
            let pricing_label = if profile.input_cost_per_million_tokens > 0.0
                || profile.output_cost_per_million_tokens > 0.0
            {
                format!(
                    "${:.4}/${:.4} per 1M",
                    profile.input_cost_per_million_tokens, profile.output_cost_per_million_tokens
                )
            } else {
                "no price".to_owned()
            };
            let extra_env_label = if profile.extra_env.is_empty() {
                "no extra env".to_owned()
            } else {
                format!("{} extra env", profile.extra_env.len())
            };
            let mapping_label = if profile.model_mappings.is_empty() {
                "no mappings".to_owned()
            } else {
                format!("{} mappings", profile.model_mappings.len())
            };
            let url_mode_label = if profile.full_url_mode {
                "full URL".to_owned()
            } else {
                profile.api_format.clone()
            };
            let profile_enabled = profile.enabled;
            let health_label = profile.health.display_label();
            let health_checking = profile.health.is_checking();
            let environment_id_for_action = environment_id.clone();
            let description_color = styles::description_font_color(true, app);
            let active_color = theme.accent();

            let row = Hoverable::new(row_mouse_state, move |state| {
                let background = if state.is_hovered() {
                    theme.surface_overlay_1()
                } else {
                    internal_colors::fg_overlay_1(theme)
                };
                let status_text = if is_active {
                    "Active"
                } else if profile_enabled {
                    "Fallback"
                } else {
                    "Disabled"
                };
                let status_color = if is_active {
                    active_color
                } else {
                    description_color
                };

                let title = Text::new_inline(
                    profile_name.clone(),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_color(theme.active_ui_text_color().into())
                .finish();

                let details = format!(
                    "{agent_name} / {environment_label} / {url_mode_label} / {base_url} / {model} / {mapping_label} / {key_label} / {priority_label} / {pricing_label} / {extra_env_label} / {health_label}"
                );
                let details = appearance
                    .ui_builder()
                    .paragraph(details)
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        font_color: Some(description_color.into()),
                        margin: Some(Coords::default().top(3.)),
                        ..Default::default()
                    })
                    .build()
                    .finish();

                let status = Text::new_inline(
                    status_text.to_owned(),
                    appearance.ui_font_family(),
                    CONTENT_FONT_SIZE,
                )
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_color(status_color.into())
                .finish();

                let edit_action =
                    AISettingsPageAction::EditCLIAgentApiProfile(edit_profile_id.clone());
                let edit_button = appearance
                    .ui_builder()
                    .button(ButtonVariant::Text, edit_mouse_state.clone())
                    .with_text_label("Edit".to_owned())
                    .with_style(UiComponentStyles {
                        font_size: Some(CONTENT_FONT_SIZE),
                        font_weight: Some(Weight::Semibold),
                        font_color: Some(description_color.into()),
                        padding: Some(Coords {
                            top: 4.,
                            bottom: 4.,
                            left: 6.,
                            right: 6.,
                        }),
                        ..Default::default()
                    })
                    .build()
                    .with_cursor(Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(edit_action.clone());
                    })
                    .finish();

                let check_action =
                    AISettingsPageAction::CheckCLIAgentApiProfile(check_profile_id.clone());
                let check_button = appearance
                    .ui_builder()
                    .button(ButtonVariant::Text, check_mouse_state.clone())
                    .with_text_label(if health_checking {
                        "Checking".to_owned()
                    } else {
                        "Check".to_owned()
                    })
                    .with_style(UiComponentStyles {
                        font_size: Some(CONTENT_FONT_SIZE),
                        font_weight: Some(Weight::Semibold),
                        font_color: Some(if health_checking {
                            active_color.into()
                        } else {
                            description_color.into()
                        }),
                        padding: Some(Coords {
                            top: 4.,
                            bottom: 4.,
                            left: 6.,
                            right: 6.,
                        }),
                        ..Default::default()
                    })
                    .build()
                    .with_cursor(Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(check_action.clone());
                    })
                    .finish();

                let toggle_action = AISettingsPageAction::SetCLIAgentApiProfileEnabled {
                    profile_id: toggle_profile_id.clone(),
                    enabled: !profile_enabled,
                };
                let toggle_button = appearance
                    .ui_builder()
                    .button(ButtonVariant::Text, toggle_mouse_state.clone())
                    .with_text_label(if profile_enabled {
                        "Disable".to_owned()
                    } else {
                        "Enable".to_owned()
                    })
                    .with_style(UiComponentStyles {
                        font_size: Some(CONTENT_FONT_SIZE),
                        font_weight: Some(Weight::Semibold),
                        font_color: Some(if profile_enabled {
                            description_color.into()
                        } else {
                            active_color.into()
                        }),
                        padding: Some(Coords {
                            top: 4.,
                            bottom: 4.,
                            left: 6.,
                            right: 6.,
                        }),
                        ..Default::default()
                    })
                    .build()
                    .with_cursor(Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(toggle_action.clone());
                    })
                    .finish();

                let remove_action =
                    AISettingsPageAction::RemoveCLIAgentApiProfile(remove_profile_id.clone());
                let remove_button = appearance
                    .ui_builder()
                    .close_button(16., remove_mouse_state.clone())
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(remove_action.clone());
                    })
                    .finish();

                Container::new(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_children([
                            Shrinkable::new(
                                1.,
                                Flex::column()
                                    .with_spacing(2.)
                                    .with_child(title)
                                    .with_child(details)
                                    .finish(),
                            )
                            .finish(),
                            Flex::row()
                                .with_spacing(8.)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_child(status)
                                .with_child(edit_button)
                                .with_child(check_button)
                                .with_child(toggle_button)
                                .with_child(remove_button)
                                .finish(),
                        ])
                        .finish(),
                )
                .with_uniform_padding(12.)
                .with_background(background)
                .with_border(Border::all(1.).with_border_fill(theme.outline()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .finish()
            });
            let row = if profile_enabled {
                row.with_cursor(Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(
                            AISettingsPageAction::SetActiveCLIAgentApiProfile {
                                agent,
                                environment_id: environment_id_for_action.clone(),
                                profile_id: profile_id_for_row.clone(),
                            },
                        );
                    })
            } else {
                row
            }
            .finish();

            list.add_child(row);
        }

        list.finish()
    }
}

impl SettingsWidget for CLIAgentApiProfilesWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "agent api profile provider switch local ssh remote claude codex gemini opencode hermes base url key model"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let description = appearance
            .ui_builder()
            .paragraph(
                "管理自定义模型供应商。启用接管后，本地和 SSH remote 的 Agent 端点会统一注入并可直接使用。",
            )
            .with_style(UiComponentStyles {
                font_size: Some(appearance.ui_font_size()),
                font_color: Some(styles::description_font_color(true, app).into()),
                margin: Some(
                    Coords::default()
                        .bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                        .right(styles::TOGGLE_WIDTH_MARGIN),
                ),
                ..Default::default()
            })
            .build()
            .finish();
        let ai_settings = AISettings::as_ref(app);
        let takeover_toggle = build_toggle_element(
            render_body_item_label::<AISettingsPageAction>(
                "启用 Agent API 接管".to_owned(),
                Some(styles::header_font_color(true, app)),
                None,
                LocalOnlyIconState::for_setting(
                    CLIAgentApiTakeoverEnabled::storage_key(),
                    CLIAgentApiTakeoverEnabled::sync_to_cloud(),
                    &mut view.local_only_icon_tooltip_states.borrow_mut(),
                    app,
                ),
                ToggleState::Enabled,
                appearance,
            ),
            render_ai_feature_switch(
                view.cli_agent_api_takeover_toggle.clone(),
                ai_settings.is_cli_agent_api_takeover_enabled(),
                true,
                AISettingsPageAction::ToggleCLIAgentApiTakeover,
                app,
            ),
            appearance,
            Some(
                "关闭时，Agentwarp 不接管本机或 SSH remote 中的 Agent API 配置；模型切换只使用 CLI 原生配置。"
                    .to_owned(),
            ),
        );

        Flex::column()
            .with_spacing(12.)
            .with_child(
                build_sub_header(
                    appearance,
                    "模型供应商",
                    Some(styles::header_font_color(true, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(description)
            .with_child(takeover_toggle)
            .with_child(self.render_provider_manager(view, appearance, app))
            .finish()
    }
}

#[derive(Default)]
struct CLIAgentSessionIdsWidget;

impl CLIAgentSessionIdsWidget {
    fn record_matches_query(
        record: &crate::workspace::view::agent_sessions::AgentSessionRecord,
        query: &str,
    ) -> bool {
        if query.is_empty() {
            return true;
        }

        let haystack = format!(
            "{} {} {} {} {}",
            record.agent.display_name(),
            record.title,
            record.agent_session_id.as_deref().unwrap_or_default(),
            record.project_path.display(),
            record.environment_id
        )
        .to_lowercase();
        haystack.contains(query)
    }

    fn render_record_row(
        record: &crate::workspace::view::agent_sessions::AgentSessionRecord,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let active_color = theme.active_ui_text_color();
        let description_color = styles::description_font_color(true, app);
        let session_id = record
            .agent_session_id
            .as_deref()
            .unwrap_or("No resume id");
        let details = format!(
            "{}  ·  {}  ·  {}",
            session_id,
            record.project_path.display(),
            record.status.label()
        );
        let title = appearance
            .ui_builder()
            .span(record.title.clone())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_weight: Some(Weight::Semibold),
                font_color: Some(active_color.into()),
                ..Default::default()
            })
            .build()
            .finish();
        let details = appearance
            .ui_builder()
            .paragraph(details)
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_color: Some(description_color.into()),
                margin: Some(Coords::default().top(2.)),
                ..Default::default()
            })
            .build()
            .finish();
        let edit_action = AISettingsPageAction::EditAgentSessionId(record.id.clone());
        let edit_button = appearance
            .ui_builder()
            .button(ButtonVariant::Text, MouseStateHandle::default())
            .with_text_label("Edit".to_owned())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_weight: Some(Weight::Semibold),
                font_color: Some(active_color.into()),
                padding: Some(Coords {
                    top: 4.,
                    bottom: 4.,
                    left: 6.,
                    right: 6.,
                }),
                ..Default::default()
            })
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(edit_action.clone());
            })
            .finish();
        let remove_action = AISettingsPageAction::RemoveAgentSessionId(record.id.clone());
        let remove_button = appearance
            .ui_builder()
            .close_button(16., MouseStateHandle::default())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(remove_action.clone());
            })
            .finish();

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_children([
                    Shrinkable::new(
                        1.,
                        Flex::column()
                            .with_spacing(2.)
                            .with_child(title)
                            .with_child(details)
                            .finish(),
                    )
                    .finish(),
                    Flex::row()
                        .with_spacing(8.)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(edit_button)
                        .with_child(remove_button)
                        .finish(),
                ])
                .finish(),
        )
        .with_uniform_padding(12.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish()
    }

    fn render_editor_field(
        label: &str,
        editor: &ViewHandle<EditorView>,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let label = appearance
            .ui_builder()
            .span(label.to_owned())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_color: Some(styles::description_font_color(true, app).into()),
                ..Default::default()
            })
            .build()
            .finish();
        Flex::column()
            .with_spacing(4.)
            .with_child(label)
            .with_child(
                ConstrainedBox::new(ChildView::new(editor).finish())
                    .with_max_width(260.)
                    .finish(),
            )
            .finish()
    }

    fn render_text_button(
        label: &str,
        action: AISettingsPageAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        appearance
            .ui_builder()
            .button(ButtonVariant::Text, MouseStateHandle::default())
            .with_text_label(label.to_owned())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                font_weight: Some(Weight::Semibold),
                font_color: Some(theme.active_ui_text_color().into()),
                padding: Some(Coords {
                    top: 6.,
                    bottom: 6.,
                    left: 8.,
                    right: 8.,
                }),
                ..Default::default()
            })
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish()
    }
}

impl SettingsWidget for CLIAgentSessionIdsWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "session ids resume id history codex claude opencode agent sessions restore"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let query = view
            .agent_session_id_search_editor
            .as_ref(app)
            .buffer_text(app)
            .trim()
            .to_lowercase();
        let records = AgentSessionsModel::as_ref(app).records();
        let agents = [CLIAgent::Codex, CLIAgent::Claude, CLIAgent::OpenCode];
        let save_label = if view.agent_session_id_editing_record_id.is_some() {
            "Save changes"
        } else {
            "Add session"
        };
        let mut column = Flex::column()
            .with_spacing(12.)
            .with_child(
                build_sub_header(
                    appearance,
                    "Session IDs",
                    Some(styles::header_font_color(true, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(
                appearance
                    .ui_builder()
                    .paragraph(
                        "Manage local resume identifiers used when Agentwarp restarts a CLI agent to apply model, permission, or reasoning changes.",
                    )
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        font_color: Some(styles::description_font_color(true, app).into()),
                        margin: Some(
                            Coords::default()
                                .bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                                .right(styles::TOGGLE_WIDTH_MARGIN),
                        ),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_child(
                Container::new(
                    Flex::column()
                        .with_spacing(10.)
                        .with_child(
                            Flex::row()
                                .with_spacing(10.)
                                .with_child(Self::render_editor_field(
                                    "Agent",
                                    &view.agent_session_id_agent_editor,
                                    appearance,
                                    app,
                                ))
                                .with_child(Self::render_editor_field(
                                    "Project path",
                                    &view.agent_session_id_project_editor,
                                    appearance,
                                    app,
                                ))
                                .finish(),
                        )
                        .with_child(
                            Flex::row()
                                .with_spacing(10.)
                                .with_child(Self::render_editor_field(
                                    "Session ID",
                                    &view.agent_session_id_value_editor,
                                    appearance,
                                    app,
                                ))
                                .with_child(Self::render_editor_field(
                                    "Title",
                                    &view.agent_session_id_title_editor,
                                    appearance,
                                    app,
                                ))
                                .finish(),
                        )
                        .with_child(
                            Flex::row()
                                .with_spacing(8.)
                                .with_child(Self::render_text_button(
                                    save_label,
                                    AISettingsPageAction::SaveAgentSessionId,
                                    appearance,
                                ))
                                .with_child(Self::render_text_button(
                                    "Cancel",
                                    AISettingsPageAction::CancelEditAgentSessionId,
                                    appearance,
                                ))
                                .finish(),
                        )
                        .finish(),
                )
                .with_uniform_padding(12.)
                .with_background(appearance.theme().surface_2())
                .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .finish(),
            )
            .with_child(
                ConstrainedBox::new(
                    ChildView::new(&view.agent_session_id_search_editor).finish(),
                )
                .with_max_width(360.)
                .finish(),
            );

        let mut any = false;
        for agent in agents {
            let mut agent_records = records
                .iter()
                .filter(|record| {
                    record.agent == agent && Self::record_matches_query(record, &query)
                })
                .collect::<Vec<_>>();
            agent_records.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
            if agent_records.is_empty() {
                continue;
            }
            any = true;
            column.add_child(
                appearance
                    .ui_builder()
                    .span(agent.display_name().to_owned())
                    .with_style(UiComponentStyles {
                        font_size: Some(CONTENT_FONT_SIZE),
                        font_weight: Some(Weight::Semibold),
                        font_color: Some(styles::header_font_color(true, app).into()),
                        margin: Some(Coords::default().top(6.)),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );
            for record in agent_records {
                column.add_child(Self::render_record_row(record, appearance, app));
            }
        }

        let mut other_records = records
            .iter()
            .filter(|record| {
                !agents.contains(&record.agent) && Self::record_matches_query(record, &query)
            })
            .collect::<Vec<_>>();
        other_records.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        if !other_records.is_empty() {
            any = true;
            column.add_child(
                appearance
                    .ui_builder()
                    .span("Other".to_owned())
                    .with_style(UiComponentStyles {
                        font_size: Some(CONTENT_FONT_SIZE),
                        font_weight: Some(Weight::Semibold),
                        font_color: Some(styles::header_font_color(true, app).into()),
                        margin: Some(Coords::default().top(6.)),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );
            for record in other_records {
                column.add_child(Self::render_record_row(record, appearance, app));
            }
        }

        if !any {
            column.add_child(
                appearance
                    .ui_builder()
                    .paragraph("No matching session IDs.")
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        font_color: Some(styles::description_font_color(true, app).into()),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );
        }

        column.finish()
    }
}

#[derive(Default)]
struct CLIAgentBuiltinPromptsWidget;

impl SettingsWidget for CLIAgentBuiltinPromptsWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "system prompts built in prompts custom prompt append replace claude code codex opencode coding agent"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut column = Flex::column()
            .with_spacing(12.)
            .with_child(
                build_sub_header(
                    appearance,
                    "System prompts",
                    Some(styles::header_font_color(true, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(
                appearance
                    .ui_builder()
                    .paragraph(
                        "Configure custom instructions for third-party coding agents. Vendor default prompts are embedded or generated by each CLI, so Warp shows inspection status separately from your custom prompt text.",
                    )
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        font_color: Some(styles::description_font_color(true, app).into()),
                        margin: Some(
                            Coords::default()
                                .bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
                                .right(styles::TOGGLE_WIDTH_MARGIN),
                        ),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );

        for (idx, agent) in AISettings::cli_agent_builtin_prompt_agents()
            .into_iter()
            .enumerate()
        {
            let Some(editor) = view.cli_agent_builtin_prompt_editors.get(idx) else {
                continue;
            };
            let Some(mode_dropdown) = view.cli_agent_builtin_prompt_mode_dropdowns.get(idx) else {
                continue;
            };

            let prompt_setting = AISettings::as_ref(app).cli_agent_builtin_prompt(agent);
            let agent_name = appearance
                .ui_builder()
                .span(agent.display_name().to_string())
                .with_style(UiComponentStyles {
                    font_size: Some(CONTENT_FONT_SIZE),
                    font_color: Some(theme.active_ui_text_color().into()),
                    ..Default::default()
                })
                .build()
                .finish();
            let mode_description = appearance
                .ui_builder()
                .paragraph(prompt_setting.mode.description())
                .with_style(UiComponentStyles {
                    font_size: Some(appearance.ui_font_size()),
                    font_color: Some(styles::description_font_color(true, app).into()),
                    margin: Some(Coords::default().top(2.).bottom(6.)),
                    ..Default::default()
                })
                .build()
                .finish();
            let default_prompt_status = appearance
                .ui_builder()
                .paragraph(AISettings::cli_agent_default_prompt_status(agent))
                .with_style(UiComponentStyles {
                    font_size: Some(appearance.ui_font_size()),
                    font_color: Some(styles::description_font_color(true, app).into()),
                    margin: Some(Coords::default().top(2.)),
                    ..Default::default()
                })
                .build()
                .finish();
            let application_status = appearance
                .ui_builder()
                .paragraph(AISettings::cli_agent_builtin_prompt_application_status(
                    agent,
                ))
                .with_style(UiComponentStyles {
                    font_size: Some(appearance.ui_font_size()),
                    font_color: Some(styles::description_font_color(true, app).into()),
                    margin: Some(Coords::default().top(2.).bottom(6.)),
                    ..Default::default()
                })
                .build()
                .finish();

            let header = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_children([
                    Shrinkable::new(1., agent_name).finish(),
                    ChildView::new(mode_dropdown).finish(),
                ])
                .finish();

            let editor_box = Container::new(
                ConstrainedBox::new(ChildView::new(editor).finish())
                    .with_height(96.)
                    .finish(),
            )
            .with_background(theme.surface_1())
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .with_horizontal_padding(8.)
            .with_vertical_padding(6.)
            .finish();

            let section = Container::new(
                Flex::column()
                    .with_spacing(4.)
                    .with_child(header)
                    .with_child(mode_description)
                    .with_child(default_prompt_status)
                    .with_child(application_status)
                    .with_child(editor_box)
                    .finish(),
            )
            .with_vertical_padding(10.)
            .with_border(Border::bottom(1.).with_border_fill(theme.outline()))
            .finish();

            column.add_child(section);
        }

        column.finish()
    }
}

/// The presentation state of the agent attribution toggle, derived from the
/// org-level [`AdminEnablementSetting`], the user's stored preference, and
/// whether AI is globally enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentAttributionToggleState {
    /// Whether the toggle is rendered in the checked state.
    pub(crate) is_enabled: bool,
    /// Whether the org has forced the value (locking the toggle with a tooltip).
    pub(crate) is_forced_by_org: bool,
    /// Whether the toggle should be rendered as non-interactive overall
    /// (forced by the org, or AI globally disabled).
    pub(crate) is_disabled: bool,
}

/// Derive the toggle state from its three inputs.
pub(crate) fn derive_agent_attribution_toggle_state(
    org_setting: &AdminEnablementSetting,
    user_pref: bool,
    is_any_ai_enabled: bool,
) -> AgentAttributionToggleState {
    let is_forced_by_org = match org_setting {
        AdminEnablementSetting::Enable | AdminEnablementSetting::Disable => true,
        AdminEnablementSetting::RespectUserSetting => false,
    };
    let is_enabled = match org_setting {
        AdminEnablementSetting::Enable => true,
        AdminEnablementSetting::Disable => false,
        AdminEnablementSetting::RespectUserSetting => user_pref,
    };
    AgentAttributionToggleState {
        is_enabled,
        is_forced_by_org,
        is_disabled: is_forced_by_org || !is_any_ai_enabled,
    }
}

#[derive(Default)]
struct AgentAttributionWidget {
    toggle: SwitchStateHandle,
}

impl SettingsWidget for AgentAttributionWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "agent attribution commit pull request co-author author credit oz warp"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let org_setting = UserWorkspaces::as_ref(app).get_agent_attribution_setting();
        let state = derive_agent_attribution_toggle_state(
            &org_setting,
            *ai_settings.agent_attribution_enabled,
            is_any_ai_enabled,
        );

        let ui_builder = appearance.ui_builder();
        let toggle = if state.is_forced_by_org {
            ui_builder
                .switch(self.toggle.clone())
                .check(state.is_enabled)
                .with_tooltip(TooltipConfig {
                    text: "This option is enforced by your organization's settings and cannot be customized.".to_string(),
                    styles: ui_builder.default_tool_tip_styles(),
                })
                .disable()
                .build()
                .finish()
        } else if !is_any_ai_enabled {
            ui_builder
                .switch(self.toggle.clone())
                .check(state.is_enabled)
                .with_disabled(true)
                .build()
                .finish()
        } else {
            ui_builder
                .switch(self.toggle.clone())
                .check(state.is_enabled)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AISettingsPageAction::ToggleAgentAttribution);
                })
                .finish()
        };

        let toggle_row = build_toggle_element(
            render_body_item_label::<AISettingsPageAction>(
                "Enable agent attribution".to_string(),
                Some(styles::header_font_color(!state.is_disabled, app)),
                None,
                LocalOnlyIconState::Hidden,
                ToggleState::Enabled,
                appearance,
            ),
            toggle,
            appearance,
            None,
        );

        Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "Agent Attribution",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(toggle_row)
            .with_child(render_ai_setting_description(
                "Oz can add attribution to commit messages and pull requests it creates",
                !state.is_disabled,
                app,
            ))
            .finish()
    }
}

#[cfg(test)]
#[path = "ai_page_tests.rs"]
mod tests;

#[derive(Default)]
struct CloudAgentComputerUseWidget {
    toggle: SwitchStateHandle,
}

impl SettingsWidget for CloudAgentComputerUseWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "oz cloud agent computer use orchestration multi-agent"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        use crate::ai::execution_profiles::{
            resolve_cloud_agent_computer_use_state, CloudAgentComputerUseState,
        };

        let is_any_ai_enabled = AISettings::as_ref(app).is_any_ai_enabled(app);

        // Determine toggle state based on workspace autonomy setting and user preference
        let CloudAgentComputerUseState {
            enabled: is_checked,
            is_forced_by_org,
        } = resolve_cloud_agent_computer_use_state(app);

        // Toggle is disabled if forced by org settings OR if AI is globally disabled
        let is_disabled = is_forced_by_org || !is_any_ai_enabled;

        let ui_builder = appearance.ui_builder();
        let toggle = if is_forced_by_org {
            // Disabled by organization setting - show tooltip on hover
            ui_builder
                .switch(self.toggle.clone())
                .check(is_checked)
                .with_tooltip(TooltipConfig {
                    text: "This option is enforced by your organization's settings and cannot be customized.".to_string(),
                    styles: ui_builder.default_tool_tip_styles(),
                })
                .disable()
                .build()
                .finish()
        } else if !is_any_ai_enabled {
            // Disabled because AI is off globally - no tooltip needed
            ui_builder
                .switch(self.toggle.clone())
                .check(is_checked)
                .with_disabled(true)
                .build()
                .finish()
        } else {
            // Enabled - allow toggling
            ui_builder
                .switch(self.toggle.clone())
                .check(is_checked)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AISettingsPageAction::ToggleCloudAgentComputerUse);
                })
                .finish()
        };

        let toggle_row = build_toggle_element(
            render_body_item_label::<AISettingsPageAction>(
                "Computer use in Cloud Agents".to_string(),
                Some(styles::header_font_color(!is_disabled, app)),
                None,
                LocalOnlyIconState::Hidden,
                ToggleState::Enabled,
                appearance,
            ),
            toggle,
            appearance,
            None,
        );

        Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "Experimental",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(toggle_row)
            .with_child(render_ai_setting_description(
                "Enable computer use in cloud agent conversations started from the Warp app.",
                !is_disabled,
                app,
            ))
            .finish()
    }
}

#[derive(Default)]
struct CloudHandoffWidget {
    handoff_toggle: SwitchStateHandle,
    auto_handoff_on_sleep_toggle: SwitchStateHandle,
    ampersand_toggle: SwitchStateHandle,
}

impl SettingsWidget for CloudHandoffWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "cloud handoff auto sleep ampersand & move to cloud local"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        FeatureFlag::OzHandoff.is_enabled() && FeatureFlag::HandoffLocalCloud.is_enabled()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        use crate::settings::PrivacySettings;

        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);

        let privacy = PrivacySettings::as_ref(app);
        let cloud_convos_off = !privacy.is_cloud_conversation_storage_enabled
            || matches!(
                UserWorkspaces::as_ref(app).get_cloud_conversation_storage_enablement_setting(),
                AdminEnablementSetting::Disable
            );
        let is_force_disabled = !is_any_ai_enabled || cloud_convos_off;

        let tooltip_text = if cloud_convos_off {
            "Cloud handoff requires cloud conversations to be enabled."
        } else {
            ""
        };

        let ui_builder = appearance.ui_builder();

        let handoff_toggle = if is_force_disabled {
            let mut builder = ui_builder.switch(self.handoff_toggle.clone()).check(false);
            if !tooltip_text.is_empty() {
                builder = builder.with_tooltip(TooltipConfig {
                    text: tooltip_text.to_string(),
                    styles: ui_builder.default_tool_tip_styles(),
                });
            }
            builder.disable().build().finish()
        } else {
            ui_builder
                .switch(self.handoff_toggle.clone())
                .check(!*ai_settings.should_force_disable_cloud_handoff)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AISettingsPageAction::ToggleCloudHandoff);
                })
                .finish()
        };

        let handoff_row = build_toggle_element(
            render_body_item_label::<AISettingsPageAction>(
                "Cloud handoff".to_string(),
                Some(styles::header_font_color(!is_force_disabled, app)),
                None,
                LocalOnlyIconState::Hidden,
                ToggleState::Enabled,
                appearance,
            ),
            handoff_toggle,
            appearance,
            None,
        );

        let mut column = Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "Cloud Handoff",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(handoff_row)
            .with_child(render_ai_setting_description(
                "Hand off local agent conversations to a cloud agent.",
                !is_force_disabled,
                app,
            ));

        if ai_settings.is_cloud_handoff_enabled(app) {
            if ai_settings
                .auto_handoff_on_sleep_enabled
                .is_supported_on_current_platform()
            {
                let auto_handoff_on_sleep_toggle = ui_builder
                    .switch(self.auto_handoff_on_sleep_toggle.clone())
                    .check(*ai_settings.auto_handoff_on_sleep_enabled)
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(AISettingsPageAction::ToggleAutoHandoffOnSleep);
                    })
                    .finish();
                let auto_handoff_on_sleep_row = build_toggle_element(
                    render_body_item_label::<AISettingsPageAction>(
                        "Auto-handoff before sleep".to_string(),
                        Some(styles::header_font_color(true, app)),
                        None,
                        LocalOnlyIconState::Hidden,
                        ToggleState::Enabled,
                        appearance,
                    ),
                    auto_handoff_on_sleep_toggle,
                    appearance,
                    None,
                );
                column.add_child(auto_handoff_on_sleep_row);
                column.add_child(render_ai_setting_description(
                    "When macOS is about to sleep, automatically moves the most recently focused running local Warp Agent conversation to Cloud Mode so it can keep working.",
                    true,
                    app,
                ));
            }
            let ampersand_toggle = ui_builder
                .switch(self.ampersand_toggle.clone())
                .check(!*ai_settings.should_force_disable_ampersand_handoff)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AISettingsPageAction::ToggleAmpersandHandoff);
                })
                .finish();

            let ampersand_row = build_toggle_element(
                render_body_item_label::<AISettingsPageAction>(
                    "Use & to trigger handoff".to_string(),
                    Some(styles::header_font_color(true, app)),
                    None,
                    LocalOnlyIconState::Hidden,
                    ToggleState::Enabled,
                    appearance,
                ),
                ampersand_toggle,
                appearance,
                None,
            );

            column.add_child(ampersand_row);
            column.add_child(render_ai_setting_description(
                "Type & as the first character to enter cloud handoff compose mode.",
                true,
                app,
            ));
        }

        column.finish()
    }
}

struct ApiKeysWidget {
    openai_api_key_editor: ViewHandle<EditorView>,
    anthropic_api_key_editor: ViewHandle<EditorView>,
    google_api_key_editor: ViewHandle<EditorView>,

    can_use_warp_credits_for_fallback: SwitchStateHandle,
    upgrade_highlight_index: HighlightedHyperlink,

    custom_inference_info_tooltip: MouseStateHandle,
    custom_inference_terms_index: HighlightedHyperlink,
    description_learn_more_index: HighlightedHyperlink,
}

impl ApiKeysWidget {
    fn new(ctx: &mut ViewContext<<Self as SettingsWidget>::View>) -> Self {
        let ai_settings = AISettings::as_ref(ctx);
        let workspace_handle = UserWorkspaces::handle(ctx);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(ctx);
        let is_byo_enabled = workspace_handle.as_ref(ctx).is_byo_api_key_enabled(ctx);

        let ApiKeys {
            openai: openai_key,
            anthropic: anthropic_key,
            google: google_key,
            ..
        } = ApiKeyManager::as_ref(ctx).keys().clone();

        // A helper macro to create and configure an API key editor.  This avoids a lot
        // of code duplication and ensures consistency between the editors.
        macro_rules! create_api_key_editor {
            ($editor:ident, $key:ident, $set_func:ident, $placeholder:literal) => {
                let $editor = ctx.add_typed_action_view(move |ctx| {
                    let appearance = Appearance::handle(ctx).as_ref(ctx);
                    let options = SingleLineEditorOptions {
                        is_password: true,
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
                    let mut editor = EditorView::single_line(options, ctx);
                    editor.set_placeholder_text($placeholder, ctx);
                    if let Some(key) = &$key {
                        editor.set_buffer_text(key, ctx);
                    }
                    editor
                });
                AISettingsPageView::update_editor_interaction_state(
                    $editor.clone(),
                    is_any_ai_enabled && is_byo_enabled,
                    ctx,
                );
                ctx.subscribe_to_view(&$editor, |_, $editor, event, ctx| {
                    if matches!(event, EditorEvent::Blurred | EditorEvent::Enter) {
                        let buffer_text = $editor.as_ref(ctx).buffer_text(ctx);
                        let key = buffer_text.is_empty().not().then_some(buffer_text);
                        ApiKeyManager::handle(ctx).update(ctx, |model, ctx| {
                            model.$set_func(key, ctx);
                        });
                    }
                });
                let editor_clone = $editor.clone();
                ctx.subscribe_to_model(&workspace_handle, move |_, workspace, event, ctx| {
                    if let UserWorkspacesEvent::TeamsChanged = event {
                        let is_any_ai_enabled =
                            AISettings::handle(ctx).as_ref(ctx).is_any_ai_enabled(ctx);
                        let is_byo_enabled = workspace.as_ref(ctx).is_byo_api_key_enabled(ctx);
                        let is_enabled = is_any_ai_enabled && is_byo_enabled;
                        let has_key = !editor_clone.as_ref(ctx).is_empty(ctx);

                        // If BYO is disabled, clear the API key from the editor and storage
                        if !is_byo_enabled && has_key {
                            editor_clone.update(ctx, |editor, ctx| {
                                editor.set_buffer_text("", ctx);
                            });
                            ApiKeyManager::handle(ctx).update(ctx, |model, ctx| {
                                model.$set_func(None, ctx);
                            });
                        }

                        AISettingsPageView::update_editor_interaction_state(
                            editor_clone.clone(),
                            is_enabled,
                            ctx,
                        );
                        ctx.notify();
                    }
                })
            };
        }

        create_api_key_editor!(openai_api_key_editor, openai_key, set_openai_key, "sk-...");
        create_api_key_editor!(
            anthropic_api_key_editor,
            anthropic_key,
            set_anthropic_key,
            "sk-ant-..."
        );
        create_api_key_editor!(
            google_api_key_editor,
            google_key,
            set_google_key,
            "AIzaSy..."
        );

        Self {
            openai_api_key_editor,
            anthropic_api_key_editor,
            google_api_key_editor,

            can_use_warp_credits_for_fallback: Default::default(),
            upgrade_highlight_index: Default::default(),

            custom_inference_info_tooltip: Default::default(),
            custom_inference_terms_index: Default::default(),
            description_learn_more_index: Default::default(),
        }
    }

    fn render_api_key_input(
        &self,
        appearance: &Appearance,
        label: &'static str,
        editor: ViewHandle<EditorView>,
        is_enabled: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let padding = Some(Coords {
            top: 10.,
            bottom: 10.,
            left: 16.,
            right: 16.,
        });
        let editor_style = UiComponentStyles {
            padding,
            background: Some(appearance.theme().surface_2().into()),
            ..Default::default()
        };

        let label = Text::new_inline(label, appearance.ui_font_family(), CONTENT_FONT_SIZE)
            .with_color(styles::header_font_color(is_enabled, app).into())
            .finish();

        let input = appearance
            .ui_builder()
            .text_input(editor)
            .with_style(editor_style)
            .build()
            .finish();

        Flex::column()
            .with_spacing(8.)
            .with_child(label)
            .with_child(input)
            .finish()
    }

    fn render_provider_key_editors(
        &self,
        appearance: &Appearance,
        is_enabled: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let mut column = Flex::column().with_spacing(16.);
        column.add_child(self.render_api_key_input(
            appearance,
            "OpenAI API key",
            self.openai_api_key_editor.clone(),
            is_enabled,
            app,
        ));
        column.add_child(self.render_api_key_input(
            appearance,
            "Anthropic API key",
            self.anthropic_api_key_editor.clone(),
            is_enabled,
            app,
        ));
        column.add_child(self.render_api_key_input(
            appearance,
            "Google API key",
            self.google_api_key_editor.clone(),
            is_enabled,
            app,
        ));
        column.finish()
    }

    fn render_custom_inference_description(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let text_fragments = vec![
            FormattedTextFragment::plain_text(
                "Use your own API keys from model providers for Warp Agent. You can also add custom endpoints to use third-party models. Custom endpoints must support the OpenAI-compatible Chat Completions API. API keys are stored only on your device, never on Warp's servers. They're used to make requests to your chosen model provider. Using auto models or models from providers you have not provided API keys for will consume Warp credits. ",
            ),
            FormattedTextFragment::hyperlink("Learn more", CUSTOM_INFERENCE_LEARN_MORE_URL),
        ];
        let description = FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(text_fragments)]),
            CONTENT_FONT_SIZE,
            appearance.ui_font_family(),
            appearance.ui_font_family(),
            blended_colors::text_sub(appearance.theme(), appearance.theme().surface_1()),
            self.description_learn_more_index.clone(),
        )
        .with_hyperlink_font_color(appearance.theme().accent().into_solid())
        .register_default_click_handlers(|url, ctx, _| {
            ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(url));
        });
        Container::new(description.finish())
            .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
            .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
            .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
            .finish()
    }

    fn render_custom_inference_info_icon(&self, appearance: &Appearance) -> Box<dyn Element> {
        let icon = Container::new(
            ConstrainedBox::new(
                Icon::Info
                    .to_warpui_icon(appearance.theme().active_ui_text_color())
                    .finish(),
            )
            .with_width(13.)
            .with_height(13.)
            .finish(),
        )
        .finish();

        let tooltip_text = FormattedText::new([FormattedTextLine::Line(vec![
            FormattedTextFragment::plain_text(
                "By using BYOK or custom endpoints, you agree to use them only as permitted by ",
            ),
            FormattedTextFragment::hyperlink("Warp's Terms of Service", CUSTOM_INFERENCE_TERMS_URL),
            FormattedTextFragment::plain_text(
                ". BYOK and custom endpoints are intended for individual use and small teams. Companies or organizations with more than 10 employees should use Warp Business or Enterprise.",
            ),
        ])]);
        let tooltip_background = appearance.theme().tooltip_background();

        let info_button =
            Hoverable::new(self.custom_inference_info_tooltip.clone(), move |state| {
                let mut stack = Stack::new().with_child(icon);
                if state.is_hovered() {
                    let tool_tip = ConstrainedBox::new(
                        Container::new(
                            FormattedTextElement::new(
                                tooltip_text.clone(),
                                10.,
                                appearance.ui_font_family(),
                                appearance.ui_font_family(),
                                appearance.theme().background().into_solid(),
                                self.custom_inference_terms_index.clone(),
                            )
                            .with_hyperlink_font_color(
                                appearance
                                    .theme()
                                    .accent()
                                    .on_background(
                                        ThemeFill::Solid(tooltip_background),
                                        MinimumAllowedContrast::Text,
                                    )
                                    .into(),
                            )
                            .register_default_click_handlers(|url, ctx, _| {
                                ctx.dispatch_typed_action(AISettingsPageAction::HyperlinkClick(
                                    url,
                                ));
                            })
                            .finish(),
                        )
                        .with_background_color(tooltip_background)
                        .with_vertical_padding(4.)
                        .with_horizontal_padding(8.)
                        .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                        .finish(),
                    )
                    .with_max_width(CUSTOM_INFERENCE_INFO_TOOLTIP_MAX_WIDTH)
                    .finish();
                    stack.add_positioned_child(
                        tool_tip,
                        OffsetPositioning::offset_from_parent(
                            vec2f(0., -3.),
                            ParentOffsetBounds::WindowByPosition,
                            ParentAnchor::TopMiddle,
                            ChildAnchor::BottomMiddle,
                        ),
                    );
                }
                stack.finish()
            })
            .with_cursor(Cursor::PointingHand);

        Container::new(Box::new(info_button))
            .with_margin_left(4.)
            .finish()
    }

    fn render_custom_endpoints_list(
        &self,
        view: &AISettingsPageView,
        appearance: &Appearance,
        is_enabled: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = styles::header_font_color(is_enabled, app);
        let endpoints = &ApiKeyManager::as_ref(app).keys().custom_endpoints;
        let chip_border = internal_colors::fg_overlay_3(theme);

        let mut list = Flex::column().with_spacing(12.);
        for (index, endpoint) in endpoints.iter().enumerate() {
            let model_labels = endpoint
                .models
                .iter()
                .map(|model| model.alias.clone().unwrap_or_else(|| model.name.clone()))
                .filter(|s| !s.trim().is_empty());

            let chips = super::render_model_chips(model_labels, appearance, text_color);

            let endpoint_name = Text::new_inline(
                endpoint.name.clone(),
                appearance.ui_font_family(),
                appearance.ui_font_size(),
            )
            .with_style(Properties::default().weight(Weight::Semibold))
            .with_color(text_color.into())
            .finish();

            let left = Flex::column()
                .with_spacing(8.)
                .with_child(endpoint_name)
                .with_child(chips)
                .finish();

            let edit_button = view
                .custom_endpoint_edit_buttons
                .get(index)
                .map(|button| button.as_ref(app).render(app))
                .unwrap_or_else(|| Empty::new().finish());

            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Shrinkable::new(1., left).finish())
                .with_child(edit_button)
                .finish();

            list.add_child(
                Container::new(row)
                    .with_uniform_padding(12.)
                    .with_background(internal_colors::fg_overlay_1(theme))
                    .with_border(Border::all(1.).with_border_fill(chip_border))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                    .finish(),
            );
        }
        list.finish()
    }

    fn render_warp_credit_fallback_toggle(
        &self,
        view: &AISettingsPageView,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);

        let toggle = render_ai_setting_toggle::<CanUseWarpCreditsForFallback>(
            "Warp credit fallback",
            AISettingsPageAction::ToggleCanUseWarpCreditsForFallback,
            *ai_settings.can_use_warp_credits_for_fallback,
            ai_settings.is_any_ai_enabled(app),
            self.can_use_warp_credits_for_fallback.clone(),
            &view.local_only_icon_tooltip_states,
            app,
        );

        let description = render_ai_setting_description(
            "When enabled, agent requests may be routed to one of Warp's provided models in the event of an error. Warp will prioritize using your API keys over your Warp credits.",
            ai_settings.is_any_ai_enabled(app),
            app,
        );

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }
}

impl SettingsWidget for ApiKeysWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "api keys bring your own byo openai anthropic google claude gemini gpt custom inference endpoint"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let is_byo_enabled = UserWorkspaces::as_ref(app).is_byo_api_key_enabled(app);
        let is_custom_inference_enabled =
            UserWorkspaces::as_ref(app).is_custom_inference_enabled(app);
        let provider_keys_enabled = is_any_ai_enabled && is_byo_enabled;
        let custom_inference_controls_enabled = is_any_ai_enabled && is_custom_inference_enabled;
        let custom_inference_flag_on = FeatureFlag::CustomInferenceEndpoints.is_enabled();
        let show_custom_inference = custom_inference_flag_on && is_custom_inference_enabled;

        let mut column = Flex::column().with_child(render_separator(appearance));

        if show_custom_inference {
            // Header row: "Custom inference" + info icon on left, "+ Add custom model" on right
            let header_left = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    build_sub_header(
                        appearance,
                        "Custom inference",
                        Some(styles::header_font_color(
                            custom_inference_controls_enabled,
                            app,
                        )),
                    )
                    .with_margin_bottom(0.)
                    .finish(),
                )
                .with_child(self.render_custom_inference_info_icon(appearance))
                .finish();

            let header_row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(header_left)
                .with_child(view.custom_inference_add_button.as_ref(app).render(app))
                .finish();

            column.add_child(
                Container::new(header_row)
                    .with_padding_bottom(HEADER_PADDING)
                    .finish(),
            );

            // Description with Learn more link
            column.add_child(self.render_custom_inference_description(app));
        } else {
            // Fallback: old "API Keys" header only
            column.add_child(
                build_sub_header(
                    appearance,
                    "API Keys",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            );
        }

        // Provider key editors (always visible)
        column.add_child(self.render_provider_key_editors(appearance, provider_keys_enabled, app));

        // Custom endpoints sub-label + list (only when flag on and endpoints non-empty)
        if show_custom_inference {
            let endpoints = &ApiKeyManager::as_ref(app).keys().custom_endpoints;
            if !endpoints.is_empty() {
                column.add_child(
                    Container::new(
                        Text::new_inline(
                            "Custom endpoints",
                            appearance.ui_font_family(),
                            CONTENT_FONT_SIZE,
                        )
                        .with_color(
                            styles::header_font_color(custom_inference_controls_enabled, app)
                                .into(),
                        )
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                    )
                    .with_margin_top(16.)
                    .with_margin_bottom(8.)
                    .finish(),
                );
                column.add_child(self.render_custom_endpoints_list(
                    view,
                    appearance,
                    custom_inference_controls_enabled,
                    app,
                ));
            }
        }

        // Warp credit fallback toggle (shown when BYO or custom inference is enabled)
        if is_byo_enabled || show_custom_inference {
            column.add_child(
                Container::new(self.render_warp_credit_fallback_toggle(view, app))
                    .with_margin_top(16.)
                    .finish(),
            );
        }

        // Upgrade CTA if BYOK not enabled
        if !is_byo_enabled {
            let auth_state = AuthStateProvider::as_ref(app).get();
            let upgrade_text_fragments = if let Some(team) =
                UserWorkspaces::as_ref(app).current_team()
            {
                if team.billing_metadata.customer_type == CustomerType::Enterprise {
                    vec![
                        FormattedTextFragment::hyperlink("Contact sales", "mailto:sales@warp.dev"),
                        FormattedTextFragment::plain_text(
                            " to enable bringing your own API keys on your Enterprise plan.",
                        ),
                    ]
                } else {
                    let current_user_email = auth_state.user_email().unwrap_or_default();
                    let has_admin_permissions = team.has_admin_permissions(&current_user_email);
                    let upgrade_url = UserWorkspaces::upgrade_link_for_team(team.uid);
                    if has_admin_permissions {
                        vec![
                            FormattedTextFragment::hyperlink(
                                "Upgrade to the Build plan",
                                upgrade_url,
                            ),
                            FormattedTextFragment::plain_text(" to use your own API keys."),
                        ]
                    } else {
                        vec![FormattedTextFragment::plain_text(
                            "Ask your team's admin to upgrade to the Build plan to use your own API keys.",
                        )]
                    }
                }
            } else if FeatureFlag::SoloUserByok.is_enabled()
                && auth_state.is_anonymous_or_logged_out()
            {
                vec![
                    FormattedTextFragment::hyperlink_action(
                        "Create an account",
                        AISettingsPageAction::SignupAnonymousUser,
                    ),
                    FormattedTextFragment::plain_text(" to use your own API keys."),
                ]
            } else {
                let user_id = auth_state.user_id().unwrap_or_default();
                let upgrade_url = UserWorkspaces::upgrade_link(user_id);
                vec![
                    FormattedTextFragment::hyperlink("Upgrade to the Build plan", upgrade_url),
                    FormattedTextFragment::plain_text(" to use your own API keys."),
                ]
            };

            let upgrade_text_element = FormattedTextElement::new(
                FormattedText::new([FormattedTextLine::Line(upgrade_text_fragments)]),
                appearance.ui_font_size(),
                appearance.ui_font_family(),
                appearance.ui_font_family(),
                blended_colors::text_sub(appearance.theme(), appearance.theme().surface_1()),
                self.upgrade_highlight_index.clone(),
            )
            .with_hyperlink_font_color(appearance.theme().accent().into_solid())
            .register_default_click_handlers_with_action_support(|hyperlink_lens, event, ctx| {
                match hyperlink_lens {
                    HyperlinkLens::Url(url) => {
                        ctx.open_url(url);
                    }
                    HyperlinkLens::Action(action_ref) => {
                        if let Some(action) =
                            action_ref.as_any().downcast_ref::<AISettingsPageAction>()
                        {
                            event.dispatch_typed_action(action.clone());
                        }
                    }
                }
            });

            column.add_child(Container::new(upgrade_text_element.finish()).finish());
        }

        column.finish()
    }
}

struct AwsBedrockWidget {
    aws_auth_refresh_command_editor: ViewHandle<EditorView>,
    aws_auth_refresh_profile_editor: ViewHandle<EditorView>,
    credentials_enabled_toggle: SwitchStateHandle,
    auto_login_toggle: SwitchStateHandle,
    refresh_credentials_button: ViewHandle<ActionButton>,
}

impl AwsBedrockWidget {
    fn new(ctx: &mut ViewContext<<Self as SettingsWidget>::View>) -> Self {
        let ai_settings = AISettings::as_ref(ctx);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(ctx);

        let aws_auth_refresh_command = ai_settings.aws_bedrock_auth_refresh_command.value().clone();
        let aws_auth_refresh_profile = ai_settings.aws_bedrock_profile.value().clone();
        let is_usage_enabled = is_any_ai_enabled
            && UserWorkspaces::as_ref(ctx).is_aws_bedrock_credentials_enabled(ctx);

        let aws_auth_refresh_command_editor = ctx.add_typed_action_view(move |ctx| {
            let appearance = Appearance::as_ref(ctx);
            let options = SingleLineEditorOptions {
                is_password: false,
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
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text("aws login", ctx);
            editor.set_buffer_text(&aws_auth_refresh_command, ctx);
            editor
        });
        AISettingsPageView::update_editor_interaction_state(
            aws_auth_refresh_command_editor.clone(),
            is_usage_enabled,
            ctx,
        );
        ctx.subscribe_to_view(&aws_auth_refresh_command_editor, |_, editor, event, ctx| {
            if matches!(event, EditorEvent::Blurred | EditorEvent::Enter) {
                let buffer_text = editor.as_ref(ctx).buffer_text(ctx);
                let should_reset = buffer_text.trim().is_empty();
                let value = if should_reset {
                    "aws login".to_string()
                } else {
                    buffer_text
                };
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings
                        .aws_bedrock_auth_refresh_command
                        .set_value(value, ctx);
                });
                if should_reset {
                    editor.update(ctx, |editor, ctx| {
                        editor.set_buffer_text("aws login", ctx);
                    });
                }
            }
        });

        let aws_auth_refresh_profile_editor = ctx.add_typed_action_view(move |ctx| {
            let appearance = Appearance::as_ref(ctx);
            let options = SingleLineEditorOptions {
                is_password: false,
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
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text("default", ctx);
            editor.set_buffer_text(&aws_auth_refresh_profile, ctx);
            editor
        });
        AISettingsPageView::update_editor_interaction_state(
            aws_auth_refresh_profile_editor.clone(),
            is_usage_enabled,
            ctx,
        );
        ctx.subscribe_to_view(&aws_auth_refresh_profile_editor, |_, editor, event, ctx| {
            if matches!(event, EditorEvent::Blurred | EditorEvent::Enter) {
                let buffer_text = editor.as_ref(ctx).buffer_text(ctx);
                let should_reset = buffer_text.trim().is_empty();
                let value = if should_reset {
                    "default".to_string()
                } else {
                    buffer_text
                };
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings.aws_bedrock_profile.set_value(value, ctx);
                });
                if should_reset {
                    editor.update(ctx, |editor, ctx| {
                        editor.set_buffer_text("default", ctx);
                    });
                }
            }
        });

        let refresh_credentials_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Refresh", SecondaryTheme)
                .with_icon(Icon::RefreshCw04)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AISettingsPageAction::RefreshAwsBedrockCredentials);
                })
        });
        refresh_credentials_button.update(ctx, |button, ctx| {
            button.set_disabled(!is_usage_enabled, ctx);
        });

        // Keep enablement in sync with the Global AI toggle.
        let aws_auth_refresh_command_editor_clone = aws_auth_refresh_command_editor.clone();
        let aws_auth_refresh_profile_editor_clone = aws_auth_refresh_profile_editor.clone();
        let refresh_credentials_button_clone = refresh_credentials_button.clone();
        ctx.subscribe_to_model(&AISettings::handle(ctx), move |_, _, event, ctx| {
            if matches!(
                event,
                AISettingsChangedEvent::IsAnyAIEnabled { .. }
                    | AISettingsChangedEvent::AwsBedrockCredentialsEnabled { .. }
            ) {
                let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
                let is_usage_enabled = is_any_ai_enabled
                    && UserWorkspaces::as_ref(ctx).is_aws_bedrock_credentials_enabled(ctx);

                AISettingsPageView::update_editor_interaction_state(
                    aws_auth_refresh_command_editor_clone.clone(),
                    is_usage_enabled,
                    ctx,
                );
                AISettingsPageView::update_editor_interaction_state(
                    aws_auth_refresh_profile_editor_clone.clone(),
                    is_usage_enabled,
                    ctx,
                );
                refresh_credentials_button_clone.update(ctx, |button, ctx| {
                    button.set_disabled(!is_usage_enabled, ctx);
                });

                ctx.notify();
            }
        });

        let aws_auth_refresh_command_editor_clone = aws_auth_refresh_command_editor.clone();
        let aws_auth_refresh_profile_editor_clone = aws_auth_refresh_profile_editor.clone();
        let refresh_credentials_button_clone = refresh_credentials_button.clone();
        ctx.subscribe_to_model(
            &UserWorkspaces::handle(ctx),
            move |_, workspace, event, ctx| {
                if let UserWorkspacesEvent::TeamsChanged = event {
                    let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
                    let is_usage_enabled = is_any_ai_enabled
                        && workspace
                            .as_ref(ctx)
                            .is_aws_bedrock_credentials_enabled(ctx);

                    AISettingsPageView::update_editor_interaction_state(
                        aws_auth_refresh_command_editor_clone.clone(),
                        is_usage_enabled,
                        ctx,
                    );
                    AISettingsPageView::update_editor_interaction_state(
                        aws_auth_refresh_profile_editor_clone.clone(),
                        is_usage_enabled,
                        ctx,
                    );
                    refresh_credentials_button_clone.update(ctx, |button, ctx| {
                        button.set_disabled(!is_usage_enabled, ctx);
                    });

                    ctx.notify();
                }
            },
        );

        Self {
            aws_auth_refresh_command_editor,
            aws_auth_refresh_profile_editor,
            credentials_enabled_toggle: SwitchStateHandle::default(),
            auto_login_toggle: SwitchStateHandle::default(),
            refresh_credentials_button,
        }
    }

    fn render_aws_bedrock_section(
        &self,
        appearance: &Appearance,
        app: &AppContext,
        is_bedrock_available: bool,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let user_workspaces = UserWorkspaces::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let is_section_enabled = is_any_ai_enabled && is_bedrock_available;
        let is_admin_enforced = matches!(
            user_workspaces.aws_bedrock_host_enablement_setting(),
            crate::workspaces::workspace::HostEnablementSetting::Enforce
        );
        let is_toggleable =
            is_section_enabled && user_workspaces.is_aws_bedrock_credentials_toggleable();
        let are_credentials_enabled = user_workspaces.is_aws_bedrock_credentials_enabled(app);
        let is_usage_enabled = is_section_enabled && are_credentials_enabled;
        let toggle_description = if is_admin_enforced {
            "Warp loads and sends local AWS CLI credentials for Bedrock-supported models. This setting is managed by your organization.".to_string()
        } else {
            "Warp loads and sends local AWS CLI credentials for Bedrock-supported models."
                .to_string()
        };

        let mut column = Flex::column().with_spacing(16.).with_child(
            Flex::column()
                .with_child(render_ai_setting_toggle::<AwsBedrockCredentialsEnabled>(
                    "Use AWS Bedrock credentials",
                    AISettingsPageAction::ToggleAwsBedrockCredentialsEnabled,
                    are_credentials_enabled,
                    is_toggleable,
                    self.credentials_enabled_toggle.clone(),
                    &RefCell::new(HashMap::new()),
                    app,
                ))
                .with_child(render_ai_setting_description(
                    toggle_description,
                    is_section_enabled,
                    app,
                ))
                .finish(),
        );

        /// Helper function to render the UI for an input field.
        fn render_input(
            appearance: &Appearance,
            label: &'static str,
            editor: ViewHandle<EditorView>,
            is_enabled: bool,
            app: &AppContext,
        ) -> Box<dyn Element> {
            let padding = Some(Coords {
                top: 10.,
                bottom: 10.,
                left: 16.,
                right: 16.,
            });
            let editor_style = UiComponentStyles {
                padding,
                background: Some(appearance.theme().surface_2().into()),
                ..Default::default()
            };

            let label = Text::new_inline(label, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                .with_color(styles::header_font_color(is_enabled, app).into())
                .finish();

            let input = appearance
                .ui_builder()
                .text_input(editor)
                .with_style(editor_style)
                .build()
                .finish();

            Flex::column()
                .with_spacing(8.)
                .with_child(label)
                .with_child(input)
                .finish()
        }

        fn render_credential_status_card(
            refresh_button: &ViewHandle<ActionButton>,
            appearance: &Appearance,
            are_credentials_enabled: bool,
            app: &AppContext,
        ) -> Box<dyn Element> {
            let (title_color, detail_color) = (
                styles::header_font_color(are_credentials_enabled, app),
                styles::description_font_color(are_credentials_enabled, app),
            );
            let (title_text, detail_text, icon) = ApiKeyManager::as_ref(app)
                .aws_credentials_state()
                .user_facing_components();

            let icon = Container::new(
                ConstrainedBox::new(icon.to_warpui_icon(title_color).finish())
                    .with_width(16.)
                    .with_height(16.)
                    .finish(),
            )
            .with_horizontal_padding(4.)
            .finish();

            let text_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(4.)
                .with_child(
                    Text::new_inline(title_text, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .with_color(title_color.into())
                        .finish(),
                )
                .with_child(
                    Text::new(detail_text, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                        .with_color(detail_color.into())
                        .soft_wrap(true)
                        .finish(),
                );

            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(12.)
                    .with_child(
                        Expanded::new(
                            1.,
                            Flex::row()
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_spacing(12.)
                                .with_child(icon)
                                .with_child(Expanded::new(1., text_column.finish()).finish())
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_child(ChildView::new(refresh_button).finish())
                    .finish(),
            )
            .with_uniform_padding(12.)
            .with_background(appearance.theme().surface_2())
            .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish()
        }

        column.add_child(
            Container::new(render_credential_status_card(
                &self.refresh_credentials_button,
                appearance,
                are_credentials_enabled,
                app,
            ))
            .with_margin_top(-styles::DESCRIPTION_MARGIN_BOTTOM)
            .finish(),
        );
        column.add_child(render_input(
            appearance,
            "Login Command",
            self.aws_auth_refresh_command_editor.clone(),
            is_usage_enabled,
            app,
        ));
        column.add_child(render_input(
            appearance,
            "AWS Profile",
            self.aws_auth_refresh_profile_editor.clone(),
            is_usage_enabled,
            app,
        ));

        let auto_login_enabled = *AISettings::as_ref(app).aws_bedrock_auto_login.value();

        let toggle = render_ai_setting_toggle::<AwsBedrockAutoLogin>(
            "Automatically run login command",
            AISettingsPageAction::ToggleAwsBedrockAutoLogin,
            auto_login_enabled,
            is_usage_enabled,
            self.auto_login_toggle.clone(),
            &RefCell::new(HashMap::new()),
            app,
        );
        let description = render_ai_setting_description(
            "When enabled, the login command will run automatically when AWS Bedrock credentials expire.",
            is_usage_enabled,
            app,
        );
        column.add_child(
            Flex::column()
                .with_child(toggle)
                .with_child(description)
                .finish(),
        );

        column.finish()
    }
}

impl SettingsWidget for AwsBedrockWidget {
    type View = AISettingsPageView;

    fn search_terms(&self) -> &str {
        "aws bedrock amazon credentials login profile"
    }

    fn should_render(&self, app: &AppContext) -> bool {
        // Only show if admin has enabled AWS Bedrock for the workspace
        UserWorkspaces::as_ref(app).is_aws_bedrock_available_from_workspace()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled(app);
        let is_bedrock_available =
            UserWorkspaces::as_ref(app).is_aws_bedrock_available_from_workspace();

        let column = Flex::column()
            .with_child(render_separator(appearance))
            .with_child(
                build_sub_header(
                    appearance,
                    "AWS Bedrock",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            )
            .with_child(self.render_aws_bedrock_section(appearance, app, is_bedrock_available));

        Container::new(column.finish())
            .with_margin_bottom(HEADER_PADDING)
            .finish()
    }
}

mod styles {
    use warp_core::ui::appearance::Appearance;
    use warp_core::ui::theme::Fill;
    use warpui::{AppContext, SingletonEntity};

    // Apply a negative margin to the description text so it appears closer to the main
    // settings option text.
    pub const DESCRIPTION_NEGATIVE_MARGIN_OFFSET: f32 = -12.;

    /// The space between a description and the next toggle.
    pub const DESCRIPTION_MARGIN_BOTTOM: f32 = 12.;

    /// Margin to leave for switch toggle to the right of the description subtext.
    pub const TOGGLE_WIDTH_MARGIN: f32 = 48.;

    pub fn header_font_color(is_enabled_setting: bool, app: &AppContext) -> Fill {
        let appearance = Appearance::as_ref(app);
        if is_enabled_setting {
            appearance
                .theme()
                .main_text_color(appearance.theme().surface_2())
        } else {
            appearance.theme().disabled_ui_text_color()
        }
    }

    pub fn description_font_color(is_enabled_setting: bool, app: &AppContext) -> Fill {
        let appearance = Appearance::as_ref(app);
        if is_enabled_setting {
            appearance
                .theme()
                .sub_text_color(appearance.theme().surface_1())
        } else {
            appearance.theme().disabled_ui_text_color()
        }
    }
}
