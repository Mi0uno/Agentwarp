//! CLI agent detection and configuration.
//!
//! This module provides types for detecting and working with CLI-based AI agents
//! like Claude Code, Gemini CLI, Codex, Amp, and Droid.

use std::borrow::Cow;
use std::collections::HashMap;

use ai::skills::SkillProvider;
use enum_iterator::Sequence;
use markdown_parser::parse_markdown;
use pathfinder_color::ColorU;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use warp_cli::agent::Harness;
use warp_completer::parsers::simple::top_level_command;
use warp_editor::content::buffer::Buffer;
use warp_editor::content::markdown::MarkdownStyle;
use warp_util::path::EscapeChar;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::ai::agent::{AgentReviewCommentBatch, DiffSetHunk};
use crate::ai::blocklist::CLAUDE_ORANGE;
use crate::code::editor::line::EditorLineLocation;
use crate::code_review::comments::AttachedReviewCommentTarget;
use crate::server::telemetry::CLIAgentType;
use crate::ui_components::icons::Icon;
use crate::workspaces::user_workspaces::UserWorkspaces;

/// UID for the Uber team.
/// See https://warp.metabaseapp.com/dashboard/1454?team_id=46347
const UBER_TEAM_UID: &str = "BdVbYjy9LRZcZrYBemSfAF";

/// Gemini brand blue color
pub(crate) const GEMINI_BLUE: ColorU = ColorU {
    r: 66,
    g: 133,
    b: 244,
    a: 255,
};

/// OpenAI brand color (dark gray/black)
pub(crate) const OPENAI_COLOR: ColorU = ColorU {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

/// Amp brand color (#F34E3F)
const AMP_COLOR: ColorU = ColorU {
    r: 243,
    g: 78,
    b: 63,
    a: 255,
};

/// Droid brand color (white)
const DROID_COLOR: ColorU = ColorU {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// OpenCode brand color (gray, used for contrast calculation only)
pub(crate) const OPENCODE_COLOR: ColorU = ColorU {
    r: 128,
    g: 128,
    b: 128,
    a: 255,
};

/// Copilot brand color (Copilot purple selected from https://brand.github.com/brand-identity/copilot)
const COPILOT_COLOR: ColorU = ColorU {
    r: 133,
    g: 52,
    b: 243,
    a: 255,
};

/// Pi brand color (white, monochrome logo)
const PI_COLOR: ColorU = ColorU {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Auggie brand color (white, monochrome logo)
const AUGGIE_COLOR: ColorU = ColorU {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Cursor brand color (#26251E, from official brand assets)
const CURSOR_COLOR: ColorU = ColorU {
    r: 38,
    g: 37,
    b: 30,
    a: 255,
};

/// Goose brand color (#101010, from Block's official Goose logo)
const GOOSE_COLOR: ColorU = ColorU {
    r: 16,
    g: 16,
    b: 16,
    a: 255,
};

/// Hermes brand color (Nous Research purple #7C3AED)
const HERMES_PURPLE: ColorU = ColorU {
    r: 124,
    g: 58,
    b: 237,
    a: 255,
};

/// Mistral brand orange (#FA520F)
const MISTRAL_ORANGE: ColorU = ColorU {
    r: 250,
    g: 82,
    b: 15,
    a: 255,
};

/// Represents a CLI agent (e.g., Claude Code, Gemini CLI, Codex, Amp, Droid, OpenCode, Copilot, Pi, Auggie, Cursor, Goose, Hermes, Mistral Vibe)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Sequence, Serialize, Deserialize)]
pub enum CLIAgent {
    Claude,
    Gemini,
    Codex,
    Amp,
    Droid,
    OpenCode,
    Copilot,
    Pi,
    Auggie,
    CursorCli,
    Goose,
    Hermes,
    Vibe,
    /// Represents an unknown/custom CLI agent matched by user-configured regex patterns.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentReasoningEffort {
    Auto,
    Off,
    NoReasoning,
    Low,
    Medium,
    High,
    ExtraHigh,
    Max,
    Ultracode,
}

impl AgentReasoningEffort {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Off => "Off",
            Self::NoReasoning => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::ExtraHigh => "Extra High",
            Self::Max => "Max",
            Self::Ultracode => "Ultracode",
        }
    }

    pub(crate) fn command_value(&self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Off => Some("off"),
            Self::NoReasoning => Some("none"),
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::ExtraHigh => Some("xhigh"),
            Self::Max => Some("max"),
            Self::Ultracode => Some("ultracode"),
        }
    }

    pub(crate) fn from_command_value(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "none" => Some(Self::NoReasoning),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::ExtraHigh),
            "max" => Some(Self::Max),
            "ultracode" => Some(Self::Ultracode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentReasoningEffortModelEvent;

pub struct AgentReasoningEffortModel {
    effort: AgentReasoningEffort,
}

impl Entity for AgentReasoningEffortModel {
    type Event = AgentReasoningEffortModelEvent;
}

impl SingletonEntity for AgentReasoningEffortModel {}

impl AgentReasoningEffortModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            effort: AgentReasoningEffort::Auto,
        }
    }

    pub fn effort(&self) -> AgentReasoningEffort {
        self.effort
    }

    pub fn set_effort(&mut self, effort: AgentReasoningEffort, ctx: &mut ModelContext<Self>) {
        if self.effort == effort {
            return;
        }

        self.effort = effort;
        ctx.emit(AgentReasoningEffortModelEvent);
    }
}

pub const DEFAULT_CLI_AGENT_MODEL_LABEL: &str = "Default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentPermissionMode {
    AskForApproval,
    ApproveForMe,
    FullAccess,
}

impl AgentPermissionMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AskForApproval => "询问确认",
            Self::ApproveForMe => "默认模式",
            Self::FullAccess => "完全访问",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::AskForApproval => "确认",
            Self::ApproveForMe => "默认模式",
            Self::FullAccess => "完全访问",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::AskForApproval => "编辑外部文件或使用网络前始终请求确认",
            Self::ApproveForMe => "仅在 CLI 认为操作不安全时请求确认",
            Self::FullAccess => "不使用审批提示或沙箱限制",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeOptions {
    pub model: Option<String>,
    pub permission_mode: Option<AgentPermissionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentControlWrite {
    pub text: String,
    pub delay_ms: u64,
}

impl AgentControlWrite {
    pub fn immediate(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            delay_ms: 0,
        }
    }

    pub fn delayed(delay_ms: u64, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            delay_ms,
        }
    }
}

const CLI_AGENT_PICKER_QUERY_DELAY_MS: u64 = 180;
const CLI_AGENT_PICKER_ENTER_DELAY_MS: u64 = 360;
const CLAUDE_PERMISSION_CYCLE_INPUT: &str = "\x1bm";
const CLAUDE_PERMISSION_CYCLE_BACK_INPUT: &str = "\x1b[Z";
const CLAUDE_PERMISSION_CYCLE_LEN: usize = 4;

#[derive(Debug, Clone)]
pub struct AgentRuntimeSettingsModelEvent;

pub struct AgentRuntimeSettingsModel {
    models_by_agent: HashMap<CLIAgent, String>,
    permissions_by_agent: HashMap<CLIAgent, AgentPermissionMode>,
}

impl Entity for AgentRuntimeSettingsModel {
    type Event = AgentRuntimeSettingsModelEvent;
}

impl SingletonEntity for AgentRuntimeSettingsModel {}

impl AgentRuntimeSettingsModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            models_by_agent: HashMap::new(),
            permissions_by_agent: HashMap::new(),
        }
    }

    pub fn model_label_for_with_custom_models(
        &self,
        agent: CLIAgent,
        allow_custom_models: bool,
    ) -> &str {
        self.models_by_agent
            .get(&agent)
            .filter(|model| {
                if allow_custom_models {
                    agent.supports_model(model)
                } else {
                    agent.model_options().contains(&model.as_str())
                }
            })
            .map(String::as_str)
            .unwrap_or(DEFAULT_CLI_AGENT_MODEL_LABEL)
    }

    pub fn model_label_for(&self, agent: CLIAgent) -> &str {
        self.model_label_for_with_custom_models(agent, true)
    }

    pub fn set_model(
        &mut self,
        agent: CLIAgent,
        model: impl Into<String>,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let model = model.into();
        let model = model.trim().to_owned();
        if model != DEFAULT_CLI_AGENT_MODEL_LABEL && !agent.supports_model(&model) {
            return false;
        }

        if model == DEFAULT_CLI_AGENT_MODEL_LABEL {
            if self.models_by_agent.remove(&agent).is_some() {
                ctx.emit(AgentRuntimeSettingsModelEvent);
                return true;
            }
            return false;
        }

        if self.models_by_agent.get(&agent) == Some(&model) {
            return false;
        }

        self.models_by_agent.insert(agent, model);
        ctx.emit(AgentRuntimeSettingsModelEvent);
        true
    }

    pub fn permission_mode_for(&self, agent: CLIAgent) -> Option<AgentPermissionMode> {
        if agent.permission_mode_options().is_empty() {
            return None;
        }

        self.permissions_by_agent
            .get(&agent)
            .copied()
            .filter(|mode| agent.supports_permission_mode(*mode))
            .or(Some(AgentPermissionMode::ApproveForMe))
    }

    pub fn set_permission_mode(
        &mut self,
        agent: CLIAgent,
        permission_mode: AgentPermissionMode,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if !agent.supports_permission_mode(permission_mode) {
            return false;
        }

        if self.permissions_by_agent.get(&agent) == Some(&permission_mode) {
            return false;
        }

        self.permissions_by_agent.insert(agent, permission_mode);
        ctx.emit(AgentRuntimeSettingsModelEvent);
        true
    }

    pub fn launch_options_for(&self, agent: CLIAgent) -> AgentRuntimeOptions {
        self.launch_options_for_with_custom_models(agent, true)
    }

    pub fn launch_options_for_with_custom_models(
        &self,
        agent: CLIAgent,
        allow_custom_models: bool,
    ) -> AgentRuntimeOptions {
        let model_label = self.model_label_for_with_custom_models(agent, allow_custom_models);
        AgentRuntimeOptions {
            model: (model_label != DEFAULT_CLI_AGENT_MODEL_LABEL).then(|| model_label.to_owned()),
            permission_mode: self.permission_mode_for(agent),
        }
    }
}

fn append_arg(command: &mut String, arg: impl AsRef<str>) {
    if !command.is_empty() {
        command.push(' ');
    }
    command.push_str(arg.as_ref());
}

fn shell_quote(value: &str) -> String {
    shell_words::quote(value).into_owned()
}

fn normalize_agent_control_arg(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        None
    } else {
        Some(value)
    }
}

fn cli_model_control_value(model: &str) -> &str {
    if model == DEFAULT_CLI_AGENT_MODEL_LABEL {
        "default"
    } else {
        model
    }
}

fn picker_control_sequence(command: &str, query: &str) -> Vec<AgentControlWrite> {
    vec![
        AgentControlWrite::immediate(command),
        AgentControlWrite::delayed(CLI_AGENT_PICKER_QUERY_DELAY_MS, query),
        AgentControlWrite::delayed(CLI_AGENT_PICKER_ENTER_DELAY_MS, "\n"),
    ]
}

fn codex_permission_picker_label(permission_mode: AgentPermissionMode) -> &'static str {
    match permission_mode {
        AgentPermissionMode::AskForApproval => "Read Only",
        AgentPermissionMode::ApproveForMe => "Auto",
        AgentPermissionMode::FullAccess => "Full Access",
    }
}

fn claude_permission_cycle_index(permission_mode: AgentPermissionMode) -> Option<usize> {
    match permission_mode {
        AgentPermissionMode::AskForApproval => Some(0),
        AgentPermissionMode::ApproveForMe => Some(1),
        AgentPermissionMode::FullAccess => Some(3),
    }
}

impl CLIAgent {
    /// The command prefix used to invoke this CLI agent.
    pub fn command_prefix(&self) -> &'static str {
        match self {
            CLIAgent::Claude => "claude",
            CLIAgent::Gemini => "gemini",
            CLIAgent::Codex => "codex",
            CLIAgent::Amp => "amp",
            CLIAgent::Droid => "droid",
            CLIAgent::OpenCode => "opencode",
            CLIAgent::Copilot => "copilot",
            CLIAgent::Pi => "pi",
            CLIAgent::Auggie => "auggie",
            CLIAgent::CursorCli => "agent",
            CLIAgent::Goose => "goose",
            CLIAgent::Hermes => "hermes",
            CLIAgent::Vibe => "vibe",
            CLIAgent::Unknown => "",
        }
    }

    pub fn model_options(&self) -> &'static [&'static str] {
        const CODEX_MODELS: &[&str] = &[
            DEFAULT_CLI_AGENT_MODEL_LABEL,
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "gpt-5.2",
        ];
        const CLAUDE_MODELS: &[&str] = &[
            DEFAULT_CLI_AGENT_MODEL_LABEL,
            "sonnet",
            "opus",
            "haiku",
            "claude-sonnet-4-6",
            "claude-opus-4-5",
            "claude-haiku-4-5",
        ];
        const GEMINI_MODELS: &[&str] = &[
            DEFAULT_CLI_AGENT_MODEL_LABEL,
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.0-flash",
            "gemini-1.5-pro-latest",
        ];
        const OPENCODE_MODELS: &[&str] = &[
            DEFAULT_CLI_AGENT_MODEL_LABEL,
            "openai/gpt-5.5",
            "openai/gpt-5-codex",
            "anthropic/claude-sonnet-4-6",
            "anthropic/claude-opus-4-5",
            "google/gemini-2.5-pro",
        ];

        match self {
            CLIAgent::Codex => CODEX_MODELS,
            CLIAgent::Claude => CLAUDE_MODELS,
            CLIAgent::Gemini => GEMINI_MODELS,
            CLIAgent::OpenCode => OPENCODE_MODELS,
            _ => &[DEFAULT_CLI_AGENT_MODEL_LABEL],
        }
    }

    pub fn supports_model_selection(&self) -> bool {
        self.model_options().len() > 1
    }

    pub fn supports_model(&self, model: &str) -> bool {
        let model = model.trim();
        !model.is_empty() && model != DEFAULT_CLI_AGENT_MODEL_LABEL
    }

    pub fn permission_mode_options(&self) -> &'static [AgentPermissionMode] {
        const PERMISSION_MODES: &[AgentPermissionMode] = &[
            AgentPermissionMode::AskForApproval,
            AgentPermissionMode::ApproveForMe,
            AgentPermissionMode::FullAccess,
        ];

        match self {
            CLIAgent::Codex | CLIAgent::Claude | CLIAgent::Gemini | CLIAgent::OpenCode => {
                PERMISSION_MODES
            }
            _ => &[],
        }
    }

    pub fn supports_permission_mode(&self, permission_mode: AgentPermissionMode) -> bool {
        self.permission_mode_options().contains(&permission_mode)
    }

    pub fn command_with_reasoning_effort(&self, effort: AgentReasoningEffort) -> String {
        self.command_with_runtime_options(
            effort,
            &AgentRuntimeOptions {
                model: None,
                permission_mode: None,
            },
        )
    }

    pub fn command_with_runtime_options(
        &self,
        effort: AgentReasoningEffort,
        options: &AgentRuntimeOptions,
    ) -> String {
        let prefix = self.command_prefix();
        if prefix.is_empty() {
            return String::new();
        }

        let mut command = prefix.to_owned();
        self.append_runtime_options(&mut command, effort, options);
        command
    }

    pub fn in_session_model_control_sequence(&self, model: &str) -> Option<Vec<AgentControlWrite>> {
        let model = normalize_agent_control_arg(model)?;

        match self {
            CLIAgent::Claude => Some(vec![AgentControlWrite::immediate(format!(
                "/model {}\n",
                cli_model_control_value(model)
            ))]),
            CLIAgent::Codex => Some(picker_control_sequence(
                "/model\n",
                cli_model_control_value(model),
            )),
            _ => None,
        }
    }

    pub fn in_session_permission_mode_control_sequence(
        &self,
        permission_mode: AgentPermissionMode,
        current_permission_mode: Option<AgentPermissionMode>,
    ) -> Option<Vec<AgentControlWrite>> {
        match self {
            CLIAgent::Codex => Some(picker_control_sequence(
                "/permissions\n",
                codex_permission_picker_label(permission_mode),
            )),
            CLIAgent::Claude => {
                let current_permission_mode = current_permission_mode?;
                if current_permission_mode == permission_mode {
                    return None;
                }

                let current_index = claude_permission_cycle_index(current_permission_mode)?;
                let target_index = claude_permission_cycle_index(permission_mode)?;
                let forward_steps = (target_index + CLAUDE_PERMISSION_CYCLE_LEN - current_index)
                    % CLAUDE_PERMISSION_CYCLE_LEN;
                let backward_steps = (current_index + CLAUDE_PERMISSION_CYCLE_LEN - target_index)
                    % CLAUDE_PERMISSION_CYCLE_LEN;
                if forward_steps == 0 {
                    None
                } else if backward_steps < forward_steps {
                    Some(vec![AgentControlWrite::immediate(
                        CLAUDE_PERMISSION_CYCLE_BACK_INPUT.repeat(backward_steps),
                    )])
                } else {
                    Some(vec![AgentControlWrite::immediate(
                        CLAUDE_PERMISSION_CYCLE_INPUT.repeat(forward_steps),
                    )])
                }
            }
            _ => None,
        }
    }

    pub fn in_session_reasoning_effort_command(
        &self,
        effort: AgentReasoningEffort,
        current_effort: Option<AgentReasoningEffort>,
    ) -> Option<String> {
        match self {
            CLIAgent::Claude => Some(format!("/effort {}\n", effort.command_value()?)),
            CLIAgent::Codex => self.codex_reasoning_effort_shortcut_input(effort, current_effort?),
            _ => None,
        }
    }

    fn codex_reasoning_effort_shortcut_input(
        &self,
        effort: AgentReasoningEffort,
        current_effort: AgentReasoningEffort,
    ) -> Option<String> {
        const CODEX_REASONING_ORDER: &[AgentReasoningEffort] = &[
            AgentReasoningEffort::Low,
            AgentReasoningEffort::Medium,
            AgentReasoningEffort::High,
            AgentReasoningEffort::ExtraHigh,
        ];

        let current_index = CODEX_REASONING_ORDER
            .iter()
            .position(|candidate| *candidate == current_effort)?;
        let target_index = CODEX_REASONING_ORDER
            .iter()
            .position(|candidate| *candidate == effort)?;

        match target_index.cmp(&current_index) {
            std::cmp::Ordering::Less => Some("\x1b,".repeat(current_index - target_index)),
            std::cmp::Ordering::Greater => Some("\x1b.".repeat(target_index - current_index)),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn resume_command_with_runtime_options(
        &self,
        session_id: &str,
        effort: AgentReasoningEffort,
        options: &AgentRuntimeOptions,
    ) -> Option<String> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        let mut command = match self {
            CLIAgent::Codex => "codex resume".to_owned(),
            CLIAgent::Claude => "claude".to_owned(),
            _ => return self.resume_command(session_id),
        };
        self.append_runtime_options(&mut command, effort, options);

        match self {
            CLIAgent::Codex => {
                append_arg(&mut command, shell_quote(session_id));
            }
            CLIAgent::Claude => {
                append_arg(&mut command, "--resume");
                append_arg(&mut command, shell_quote(session_id));
            }
            _ => {}
        }

        Some(command)
    }

    pub fn resume_last_command_with_runtime_options(
        &self,
        effort: AgentReasoningEffort,
        options: &AgentRuntimeOptions,
    ) -> Option<String> {
        let mut command = match self {
            CLIAgent::Codex => "codex resume".to_owned(),
            _ => return None,
        };
        self.append_runtime_options(&mut command, effort, options);
        append_arg(&mut command, "--last");
        Some(command)
    }

    fn append_runtime_options(
        &self,
        command: &mut String,
        effort: AgentReasoningEffort,
        options: &AgentRuntimeOptions,
    ) {
        if let Some(permission_mode) = options
            .permission_mode
            .filter(|mode| self.supports_permission_mode(*mode))
        {
            self.append_permission_mode(command, permission_mode);
        }

        if let Some(model) = options
            .model
            .as_deref()
            .filter(|model| self.supports_model(model))
        {
            self.append_model(command, model);
        }

        if !self.supports_reasoning_effort(effort) {
            return;
        }

        if matches!(
            (self, effort),
            (CLIAgent::Claude, AgentReasoningEffort::Ultracode)
        ) {
            append_arg(command, "--settings");
            append_arg(command, shell_quote("{\"ultracode\":true}"));
            return;
        }

        let Some(effort) = effort.command_value() else {
            return;
        };

        match self {
            CLIAgent::Claude => {
                append_arg(command, "--effort");
                append_arg(command, effort);
            }
            CLIAgent::Codex => {
                append_arg(command, "-c");
                append_arg(command, format!("model_reasoning_effort={effort}"));
            }
            CLIAgent::Droid => {
                append_arg(command, "--reasoning-effort");
                append_arg(command, effort);
            }
            _ => {}
        }
    }

    fn append_model(&self, command: &mut String, model: &str) {
        match self {
            CLIAgent::Codex => {
                append_arg(command, "-m");
                append_arg(command, shell_quote(model));
            }
            CLIAgent::Claude | CLIAgent::Gemini | CLIAgent::OpenCode => {
                append_arg(command, "--model");
                append_arg(command, shell_quote(model));
            }
            _ => {}
        }
    }

    fn append_permission_mode(&self, command: &mut String, permission_mode: AgentPermissionMode) {
        match self {
            CLIAgent::Codex => match permission_mode {
                AgentPermissionMode::AskForApproval => {
                    append_arg(command, "--sandbox");
                    append_arg(command, "read-only");
                    append_arg(command, "--ask-for-approval");
                    append_arg(command, "untrusted");
                }
                AgentPermissionMode::ApproveForMe => {
                    append_arg(command, "--sandbox");
                    append_arg(command, "workspace-write");
                    append_arg(command, "--ask-for-approval");
                    append_arg(command, "on-request");
                }
                AgentPermissionMode::FullAccess => {
                    append_arg(command, "--dangerously-bypass-approvals-and-sandbox");
                }
            },
            CLIAgent::Claude => match permission_mode {
                AgentPermissionMode::AskForApproval => {
                    append_arg(command, "--allow-dangerously-skip-permissions");
                    append_arg(command, "--permission-mode");
                    append_arg(command, "default");
                }
                AgentPermissionMode::ApproveForMe => {
                    append_arg(command, "--allow-dangerously-skip-permissions");
                    append_arg(command, "--permission-mode");
                    append_arg(command, "acceptEdits");
                }
                AgentPermissionMode::FullAccess => {
                    append_arg(command, "--allow-dangerously-skip-permissions");
                    append_arg(command, "--permission-mode");
                    append_arg(command, "bypassPermissions");
                }
            },
            CLIAgent::Gemini => match permission_mode {
                AgentPermissionMode::AskForApproval => {
                    append_arg(command, "--approval-mode");
                    append_arg(command, "default");
                }
                AgentPermissionMode::ApproveForMe => {
                    append_arg(command, "--approval-mode");
                    append_arg(command, "auto_edit");
                }
                AgentPermissionMode::FullAccess => {
                    append_arg(command, "--approval-mode");
                    append_arg(command, "yolo");
                }
            },
            CLIAgent::OpenCode => {
                let permission_config = match permission_mode {
                    AgentPermissionMode::AskForApproval => "{\"*\":\"ask\"}",
                    AgentPermissionMode::ApproveForMe => {
                        "{\"read\":\"allow\",\"glob\":\"allow\",\"grep\":\"allow\",\"list\":\"allow\",\"lsp\":\"allow\",\"bash\":\"ask\",\"edit\":\"ask\",\"webfetch\":\"ask\",\"websearch\":\"ask\",\"external_directory\":\"ask\"}"
                    }
                    AgentPermissionMode::FullAccess => "\"allow\"",
                };
                *command = format!(
                    "OPENCODE_PERMISSION={} {command}",
                    shell_quote(permission_config)
                );
            }
            _ => {}
        }
    }

    pub fn reasoning_effort_options(&self) -> &'static [AgentReasoningEffort] {
        const CODEX_OPTIONS: &[AgentReasoningEffort] = &[
            AgentReasoningEffort::Low,
            AgentReasoningEffort::Medium,
            AgentReasoningEffort::High,
            AgentReasoningEffort::ExtraHigh,
        ];
        const CLAUDE_OPTIONS: &[AgentReasoningEffort] = &[
            AgentReasoningEffort::Low,
            AgentReasoningEffort::Medium,
            AgentReasoningEffort::High,
            AgentReasoningEffort::ExtraHigh,
            AgentReasoningEffort::Max,
            AgentReasoningEffort::Ultracode,
        ];
        const DROID_OPTIONS: &[AgentReasoningEffort] = &[
            AgentReasoningEffort::Off,
            AgentReasoningEffort::NoReasoning,
            AgentReasoningEffort::Low,
            AgentReasoningEffort::Medium,
            AgentReasoningEffort::High,
        ];

        match self {
            CLIAgent::Claude => CLAUDE_OPTIONS,
            CLIAgent::Codex => CODEX_OPTIONS,
            CLIAgent::Droid => DROID_OPTIONS,
            _ => &[],
        }
    }

    pub fn supports_reasoning_effort(&self, effort: AgentReasoningEffort) -> bool {
        effort == AgentReasoningEffort::Auto || self.reasoning_effort_options().contains(&effort)
    }

    pub fn default_reasoning_effort(&self) -> Option<AgentReasoningEffort> {
        let options = self.reasoning_effort_options();
        if options.contains(&AgentReasoningEffort::High) {
            Some(AgentReasoningEffort::High)
        } else {
            options.first().copied()
        }
    }

    /// Whether this agent supports resuming a prior local CLI-agent session by id.
    pub fn supports_resume(&self) -> bool {
        matches!(self, CLIAgent::Claude | CLIAgent::Codex)
    }

    /// Returns the command used to resume a prior local CLI-agent session when the agent supports
    /// resuming by session id.
    pub fn resume_command(&self, session_id: &str) -> Option<String> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        let session_id = shell_words::quote(session_id);
        match self {
            CLIAgent::Claude => Some(format!("{} --resume {session_id}", self.command_prefix())),
            CLIAgent::Codex => Some(format!("{} resume {session_id}", self.command_prefix())),
            _ => None,
        }
    }

    /// Serialized version of the CLIAgent name (e.g. "Claude", "Gemini"). Used for the
    /// session-sharing protocol's opaque `cli_agent` string field.
    pub fn to_serialized_name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// Inverse of `to_serialized_name`. Falls back to `Unknown`.
    pub fn from_serialized_name(name: &str) -> CLIAgent {
        serde_json::from_value(name.into()).unwrap_or(CLIAgent::Unknown)
    }

    /// Returns the [`CLIAgent`] corresponding to a cloud-agent [`Harness`] when it represents a
    /// third-party agent. Returns `None` for [`Harness::Oz`] (Warp's built-in harness has no
    /// distinct CLI agent identity).
    pub fn from_harness(harness: Harness) -> Option<Self> {
        match harness {
            Harness::Oz => None,
            Harness::Claude => Some(CLIAgent::Claude),
            Harness::Gemini => Some(CLIAgent::Gemini),
            Harness::OpenCode => Some(CLIAgent::OpenCode),
            Harness::Codex => Some(CLIAgent::Codex),
            Harness::Unknown => Some(CLIAgent::Unknown),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CLIAgent::Claude => "Claude Code",
            CLIAgent::Gemini => "Gemini",
            CLIAgent::Codex => "Codex",
            CLIAgent::Amp => "Amp",
            CLIAgent::Droid => "Droid",
            CLIAgent::OpenCode => "OpenCode",
            CLIAgent::Copilot => "Copilot",
            CLIAgent::Pi => "Pi",
            CLIAgent::Auggie => "Auggie",
            CLIAgent::CursorCli => "Cursor",
            CLIAgent::Goose => "Goose",
            CLIAgent::Hermes => "Hermes",
            CLIAgent::Vibe => "Mistral Vibe",
            CLIAgent::Unknown => "CLI Agent",
        }
    }

    /// Returns the Icon for this CLI agent, or `None` for unknown/custom agents.
    pub fn icon(&self) -> Option<Icon> {
        match self {
            CLIAgent::Claude => Some(Icon::ClaudeLogo),
            CLIAgent::Gemini => Some(Icon::GeminiLogo),
            CLIAgent::Codex => Some(Icon::OpenAILogo),
            CLIAgent::Amp => Some(Icon::AmpLogo),
            CLIAgent::Droid => Some(Icon::DroidLogo),
            CLIAgent::OpenCode => Some(Icon::OpenCodeLogo),
            CLIAgent::Copilot => Some(Icon::CopilotLogo),
            CLIAgent::Pi => Some(Icon::PiLogo),
            CLIAgent::Auggie => Some(Icon::AuggieLogo),
            CLIAgent::CursorCli => Some(Icon::CursorLogo),
            CLIAgent::Goose => Some(Icon::GooseLogo),
            CLIAgent::Hermes => None,
            // Vibe is recognized but ships without a brand asset. The brand color
            // still drives the toolbar tile; an `Icon::MistralLogo` can be wired
            // up in a follow-up once an officially licensed SVG is available.
            CLIAgent::Vibe => None,
            CLIAgent::Unknown => None,
        }
    }

    /// Returns the skill providers whose skills this CLI agent can natively interpret.
    /// When the CLI agent rich input is open, only skills from these providers are shown
    /// in the slash menu. Returns an empty slice for agents with no known skills support.
    pub fn supported_skill_providers(&self) -> &'static [SkillProvider] {
        match self {
            CLIAgent::Claude => &[SkillProvider::Claude],
            CLIAgent::Codex => &[
                SkillProvider::Agents,
                SkillProvider::Claude,
                SkillProvider::Codex,
            ],
            CLIAgent::OpenCode => &[
                SkillProvider::OpenCode,
                SkillProvider::Agents,
                SkillProvider::Claude,
            ],
            CLIAgent::Gemini => &[SkillProvider::Agents, SkillProvider::Gemini],
            CLIAgent::Amp => &[SkillProvider::Agents],
            CLIAgent::Copilot => &[SkillProvider::Agents, SkillProvider::Copilot],
            CLIAgent::Droid => &[SkillProvider::Droid, SkillProvider::Agents],
            CLIAgent::Pi => &[SkillProvider::Agents],
            CLIAgent::Auggie => &[SkillProvider::Agents],
            CLIAgent::CursorCli => &[SkillProvider::Agents],
            CLIAgent::Goose => &[SkillProvider::Agents],
            CLIAgent::Hermes => &[SkillProvider::Agents],
            CLIAgent::Vibe => &[SkillProvider::Agents],
            CLIAgent::Unknown => &[],
        }
    }

    /// Returns the prefix character used for skill invocations by this CLI agent.
    /// Most agents use `/` (e.g. `/skill-name`), but Codex uses `$` (e.g. `$skill-name`).
    pub fn skill_command_prefix(&self) -> &'static str {
        match self {
            CLIAgent::Codex => "$",
            _ => "/",
        }
    }

    /// Whether this CLI agent supports the `!` bash mode prefix in the rich input.
    /// When `true`, typing `!` in the CLI agent rich input activates shell mode with
    /// decorations, completions, and error underlining.
    ///
    /// TODO(advait): Check whether Gemini, Amp, Droid, and Copilot support `!` bash
    /// mode and enable them here if so.
    pub fn supports_bash_mode(&self) -> bool {
        matches!(
            self,
            CLIAgent::Claude | CLIAgent::Codex | CLIAgent::OpenCode
        )
    }

    /// Returns the brand color for this CLI agent, or `None` for unknown/custom agents.
    pub fn brand_color(&self) -> Option<ColorU> {
        match self {
            CLIAgent::Claude => Some(CLAUDE_ORANGE),
            CLIAgent::Gemini => Some(GEMINI_BLUE),
            CLIAgent::Codex => Some(OPENAI_COLOR),
            CLIAgent::Amp => Some(AMP_COLOR),
            CLIAgent::Droid => Some(DROID_COLOR),
            CLIAgent::OpenCode => Some(OPENCODE_COLOR),
            CLIAgent::Copilot => Some(COPILOT_COLOR),
            CLIAgent::Pi => Some(PI_COLOR),
            CLIAgent::Auggie => Some(AUGGIE_COLOR),
            CLIAgent::CursorCli => Some(CURSOR_COLOR),
            CLIAgent::Goose => Some(GOOSE_COLOR),
            CLIAgent::Hermes => Some(HERMES_PURPLE),
            CLIAgent::Vibe => Some(MISTRAL_ORANGE),
            CLIAgent::Unknown => None,
        }
    }

    /// Returns the icon color to use when rendered on the brand-colored circle background.
    /// Agents with light brand colors use a dark icon for contrast.
    pub fn brand_icon_color(&self) -> ColorU {
        match self {
            CLIAgent::Pi | CLIAgent::Auggie | CLIAgent::Droid => ColorU::new(0, 0, 0, 255),
            _ => ColorU::white(),
        }
    }

    /// Extracts the first meaningful command token from a command string.
    ///
    /// When `escape_char` is provided, uses shell parsing to skip leading
    /// env-var assignments (e.g. `FOO=1 claude` → `claude`).
    /// Otherwise falls back to a simple whitespace split.
    fn extract_first_command(command: &str, escape_char: Option<EscapeChar>) -> Option<String> {
        match escape_char {
            Some(esc) => top_level_command(command, esc),
            None => command.split_whitespace().next().map(String::from),
        }
    }

    /// Detects the CLI agent from a command string.
    ///
    /// When `escape_char` is provided, full shell parsing is used to skip leading
    /// env-var assignments (e.g. `FOO=1 claude`). Otherwise falls back to a simple
    /// whitespace split.
    ///
    /// If `aliases` is provided, the first word of the command will be looked up
    /// in the alias map. If found, the alias value replaces the first word to
    /// produce the resolved command used for detection.
    ///
    /// Returns `Some(CLIAgent)` if the command matches a known CLI agent, `None` otherwise.
    pub fn detect(
        command: &str,
        escape_char: Option<EscapeChar>,
        aliases: Option<&HashMap<SmolStr, String>>,
        ctx: &AppContext,
    ) -> Option<CLIAgent> {
        let trimmed = command.trim_start();
        let first_word = Self::extract_first_command(trimmed, escape_char)?;

        // Resolve the full command through aliases. If the first word matches an
        // alias, replace it with the alias value to produce the resolved command.
        let resolved_command: Cow<'_, str> = aliases
            .and_then(|a| a.get(first_word.as_str()))
            .map(|alias_value| {
                let rest = trimmed
                    .find(first_word.as_str())
                    .map(|pos| &trimmed[pos + first_word.len()..])
                    .unwrap_or("");
                Cow::Owned(format!("{}{}", alias_value.trim(), rest))
            })
            .unwrap_or(Cow::Borrowed(trimmed));

        let resolved_first_word = Self::extract_first_command(&resolved_command, escape_char)?;

        // Check if resolved command matches any known CLI agent.
        // Also matches `aifx agent run claude` as Claude for Uber employees,
        // and the `vibe-acp` ACP-mode binary as Mistral Vibe.
        enum_iterator::all::<CLIAgent>()
            .filter(|agent| !matches!(agent, CLIAgent::Unknown))
            .find(|agent| {
                resolved_first_word == agent.command_prefix()
                    || (matches!(agent, CLIAgent::Claude)
                        && Self::is_aifx_agent_run_claude(&resolved_command, ctx))
                    || (matches!(agent, CLIAgent::Vibe) && resolved_first_word == "vibe-acp")
            })
    }

    /// Returns true if the resolved command is `aifx agent run claude` (Uber's
    /// internal wrapper around Claude) and the user is on the Uber team.
    /// We special-case this so Uber employees get the toolbar without needing
    /// to configure anything.
    fn is_aifx_agent_run_claude(resolved_command: &str, ctx: &AppContext) -> bool {
        resolved_command.starts_with("aifx agent run claude")
            && Self::is_on_uber_team(UserWorkspaces::as_ref(ctx))
    }

    fn is_on_uber_team(user_workspaces: &UserWorkspaces) -> bool {
        user_workspaces
            .workspaces()
            .iter()
            .flat_map(|workspace| workspace.teams.iter())
            .any(|team| team.uid.uid() == UBER_TEAM_UID)
    }
}

/// Builds a prompt string from a batch of code review comments suitable for
/// writing to a CLI agent's PTY.
///
/// # Location format
/// Locations use `L<line>` notation (1-indexed).
/// Line ranges are written `L<start>-L<end>` where both ends are **inclusive**.
/// Instructs the agent to run `git diff` for deleted-line context rather than
/// inlining the full diff.
pub fn build_review_prompt(review: &AgentReviewCommentBatch) -> String {
    let mut text = String::from(
        "Please address the following code review comments. \
         Run `git diff` (or `git diff HEAD`) to see the full context of any changes, \
         especially for deleted lines.\n",
    );

    for comment in &review.comments {
        if comment.outdated {
            continue;
        }
        let body = export_review_comment_for_cli_prompt(&comment.content);
        let location = match &comment.target {
            AttachedReviewCommentTarget::Line {
                absolute_file_path,
                line,
                ..
            } => {
                let path = absolute_file_path.display_path();
                match line {
                    EditorLineLocation::Current { line_number, .. } => {
                        let n = line_number.as_usize() + 1;
                        format!("{path} L{n}")
                    }
                    EditorLineLocation::Removed { line_number, .. } => {
                        let n = line_number.as_usize() + 1;
                        format!("{path} (deleted, was L{n} — see `git diff`)")
                    }
                    EditorLineLocation::Collapsed { line_range } => {
                        // line_range is [start, end) 0-indexed; convert to L<start>-L<end>
                        // where both start and end are 1-indexed inclusive.
                        let start = line_range.start.as_usize() + 1;
                        let end = line_range.end.as_usize();
                        format!("{path} (collapsed hunk, L{start}-L{end} — see `git diff`)")
                    }
                }
            }
            AttachedReviewCommentTarget::File { absolute_file_path } => {
                let path = absolute_file_path.display_path();
                let is_deleted = review.diff_set.iter().any(|(file_key, hunks)| {
                    path.ends_with(file_key.as_str())
                        && !hunks.is_empty()
                        && hunks
                            .iter()
                            .all(|h| h.lines_added == 0 && h.lines_removed > 0)
                });
                if is_deleted {
                    format!("{path} (deleted file — see `git diff`)")
                } else {
                    path
                }
            }
            AttachedReviewCommentTarget::General => "General".to_string(),
        };
        text.push_str(&format!("\n- {location}: {body}"));
    }

    text
}

fn export_review_comment_for_cli_prompt(comment: &str) -> String {
    let mut result = parse_markdown(comment)
        .map(|parsed| {
            Buffer::export_to_markdown(
                parsed,
                None,
                MarkdownStyle::Export {
                    app_context: None,
                    should_not_escape_markdown_punctuation: true,
                },
            )
        })
        .unwrap_or_else(|_| comment.to_string());
    result.truncate(result.trim_end().len());
    result
}

/// Builds a prompt string for a single diff hunk location suitable for writing
/// to a CLI agent's PTY. Includes change stats (+N -N) and instructs the agent
/// to run `git diff` for full context.
///
/// # Location format
/// `<path> L<start>-L<end>` where `start` and `end` are 1-indexed and both
/// ends are **inclusive**.
pub fn build_diff_hunk_prompt(
    file_path: &str,
    start_line: usize,
    end_line: usize,
    lines_added: u32,
    lines_removed: u32,
) -> String {
    format!(
        "{file_path} L{start_line}-L{end_line} (+{lines_added} -{lines_removed}) \
         -- run `git diff` to see the full context."
    )
}

/// Builds a prompt string for a set of diff file context hunks suitable for
/// writing to a CLI agent's PTY.
///
/// # Location format
/// Each line is `<path> L<start>-L<end> (+N -N)` where `start` and `end` are
/// 1-indexed and both ends are **inclusive**.
pub fn build_diff_context_prompt(file_diffs: &HashMap<String, Vec<DiffSetHunk>>) -> String {
    let mut text = String::new();
    let mut sorted_keys: Vec<&String> = file_diffs.keys().collect();
    sorted_keys.sort();
    for file_key in sorted_keys {
        let hunks = &file_diffs[file_key];
        for hunk in hunks {
            // hunk.line_range is [start, end) 0-indexed; convert to L<start>-L<end>
            // where both start and end are 1-indexed inclusive.
            let start = hunk.line_range.start.as_usize() + 1;
            let end = hunk.line_range.end.as_usize();
            text.push_str(&format!(
                "{file_key} L{start}-L{end} (+{} -{})",
                hunk.lines_added, hunk.lines_removed,
            ));
            text.push('\n');
        }
    }
    // Remove trailing newline.
    text.truncate(text.trim_end().len());
    text
}

/// Builds a prompt for a single-line text selection suitable for writing to a CLI agent's PTY.
/// Prefixes the literal text with its file path and line number for context.
///
/// # Format
/// `<path> L<line>: <text>` where `line` is 1-indexed.
pub fn build_selection_substring_prompt(file_path: &str, line: usize, text: &str) -> String {
    format!("{file_path} L{line}: {text}")
}

/// Builds a prompt for a multi-line selection suitable for writing to a CLI agent's PTY.
/// For single-line selections, use [`build_selection_substring_prompt`] instead.
///
/// # Location format
/// `<path> L<start>-L<end>` where line numbers are 1-indexed and both ends are inclusive.
pub fn build_selection_line_range_prompt(
    file_path: &str,
    start_line: usize,
    end_line: usize,
) -> String {
    format!("{file_path} L{start_line}-L{end_line}")
}

impl From<CLIAgent> for CLIAgentType {
    fn from(agent: CLIAgent) -> Self {
        match agent {
            CLIAgent::Claude => CLIAgentType::Claude,
            CLIAgent::Gemini => CLIAgentType::Gemini,
            CLIAgent::Codex => CLIAgentType::Codex,
            CLIAgent::Amp => CLIAgentType::Amp,
            CLIAgent::Droid => CLIAgentType::Droid,
            CLIAgent::OpenCode => CLIAgentType::OpenCode,
            CLIAgent::Copilot => CLIAgentType::Copilot,
            CLIAgent::Pi => CLIAgentType::Pi,
            CLIAgent::Auggie => CLIAgentType::Auggie,
            CLIAgent::CursorCli => CLIAgentType::Cursor,
            CLIAgent::Goose => CLIAgentType::Goose,
            CLIAgent::Hermes => CLIAgentType::Hermes,
            CLIAgent::Vibe => CLIAgentType::Vibe,
            CLIAgent::Unknown => CLIAgentType::Unknown,
        }
    }
}

#[cfg(test)]
#[path = "cli_agent_tests.rs"]
mod tests;
