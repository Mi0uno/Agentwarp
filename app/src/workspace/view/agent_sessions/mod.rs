use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
#[cfg(not(target_family = "wasm"))]
use std::fs::{self, File};
#[cfg(not(target_family = "wasm"))]
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(not(target_family = "wasm"))]
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use pathfinder_color::ColorU;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;
use serde::{Deserialize, Serialize};
use settings::Setting;
use uuid::Uuid;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::ui::Icon;
use warp_core::user_preferences::GetUserPreferences as _;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    AcceptedByDropTarget, Border, ChildAnchor, ClippedScrollStateHandle, ClippedScrollable,
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Dismiss, DragAxis, Draggable,
    DraggableState, DropTarget, DropTargetData, Element, Empty, Fill as ElementFill, Flex,
    Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning, ParentAnchor,
    ParentElement, ParentOffsetBounds, Radius, Rect, SavePosition, ScrollbarWidth, Shrinkable,
    Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::prelude::Align;
use warpui::r#async::Timer;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text::Span;
use warpui::ui_components::text_input::TextInput;
use warpui::{
    AppContext, Entity, EntityId, ModelContext, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WindowId,
};

use crate::ai_assistant::requests::GenerateDialogueResult;
use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpEscapeKey, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::projects::ProjectManagementModel;
use crate::server::server_api::ServerApiProvider;
use crate::terminal::cli_agent::{AgentReasoningEffort, AgentReasoningEffortModel, CLIAgent};
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModel,
    CLIAgentSessionsModelEvent,
};
use crate::terminal::model::block::BlockState;
use crate::terminal::view::TerminalView;
use crate::workspace::tab_settings::{SingletonAgentGroupBehavior, TabSettings};
use crate::workspace::view::ssh_remote::{
    SshRemoteConnectionStatus, SshRemoteHost, SshRemoteModel, SSH_REMOTE_LOCAL_ENVIRONMENT_ID,
};
use crate::workspace::{ActiveSession, WorkspaceAction};

const AGENT_SESSION_RECORDS_PREF_KEY: &str = "agent_sessions.records.v1";
const AGENT_SESSION_PROJECTS_PREF_KEY: &str = "agent_sessions.projects.v1";
const AGENT_SESSION_PROJECT_ORDER_PREF_KEY: &str = "agent_sessions.project_order.v1";
const AGENT_SESSION_COLLAPSED_PROJECTS_PREF_KEY: &str = "agent_sessions.collapsed_projects.v1";
const MAX_AGENT_SESSION_RECORDS: usize = 200;
const MAX_TITLE_CHARS: usize = 96;
const MAX_AUTO_TITLE_CHARS: usize = 56;
const AUTO_TITLE_REFRESH_INTERVAL_MS: i64 = 5 * 60 * 1_000;
const AUTO_TITLE_REFRESH_SCAN_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_TITLE_REFRESH_CHAR_THRESHOLD: usize = 8_000;
#[cfg(not(target_family = "wasm"))]
const AUTO_TITLE_CLI_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_HOSTED_TRANSCRIPT_CHARS: usize = 60_000;
pub(crate) const HOSTED_TRANSCRIPT_HEADER_PREFIX: &str = "--- Agentwarp saved chat history";
pub(crate) const HOSTED_TRANSCRIPT_END_MARKER: &str = "--- End saved chat history ---";
const AGENT_BUTTON_SIZE: f32 = 26.;
const SESSION_ACTION_BUTTON_SIZE: f32 = 20.;
const ACTION_MENU_WIDTH: f32 = 168.;
const ICON_BUTTON_SIZE: f32 = 22.;
const SIDEBAR_HORIZONTAL_PADDING: f32 = 12.;
const SINGLETON_GROUP_PROMPT_WIDTH: f32 = 232.;
const SINGLETON_GROUP_PROMPT_OFFSET: f32 = 10.;
const DELETE_CONFIRMATION_PROMPT_WIDTH: f32 = 260.;
const DROP_INTO_GROUP_VERTICAL_FRACTION: f32 = 0.28;
const DROP_OUT_OF_GROUP_HORIZONTAL_OFFSET: f32 = 10.;
const SSH_REMOTE_LOADING_TICK: Duration = Duration::from_millis(650);

const SUPPORTED_AGENTS: [CLIAgent; 3] = [CLIAgent::Claude, CLIAgent::Codex, CLIAgent::OpenCode];

fn default_agent_session_environment_id() -> String {
    SSH_REMOTE_LOCAL_ENVIRONMENT_ID.to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSessionStatus {
    Starting,
    InProgress,
    Success,
    Blocked,
    Unknown,
}

impl AgentSessionStatus {
    pub fn label(self) -> &'static str {
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
    #[serde(default = "default_agent_session_environment_id")]
    pub environment_id: String,
    pub project_path: PathBuf,
    pub agent: CLIAgent,
    pub title: String,
    pub status: AgentSessionStatus,
    pub agent_session_id: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub parent_agent_session_id: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub archived_at_ms: Option<i64>,
    #[serde(default)]
    pub title_overridden: bool,
    #[serde(default)]
    pub auto_title_fingerprint: Option<u64>,
    #[serde(default)]
    pub auto_title_summarized_at_ms: Option<i64>,
    #[serde(default)]
    pub auto_title_source_chars: usize,
    #[serde(default)]
    pub hosted_transcript: Option<String>,
    #[serde(default)]
    pub hosted_transcript_updated_at_ms: Option<i64>,
    #[serde(skip, default)]
    pub terminal_view_id: Option<EntityId>,
    #[serde(skip, default)]
    pub group_terminal_view_id: Option<EntityId>,
}

impl AgentSessionRecord {
    fn is_archived(&self) -> bool {
        self.archived_at_ms.is_some()
    }

    #[cfg(test)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSessionProjectRecord {
    #[serde(default = "default_agent_session_environment_id")]
    environment_id: String,
    project_path: PathBuf,
    updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct AgentSessionsModelEvent;

pub struct AgentSessionsModel {
    records: Vec<AgentSessionRecord>,
    project_paths: Vec<AgentSessionProjectRecord>,
    pending_title_generations: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionMovePlacement {
    Before,
    After,
    IntoGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentProjectMovePlacement {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionMoveOutcome {
    pub expanded_group_id: Option<String>,
    pub singleton_group_id: Option<String>,
    pub terminal_view_ids_to_detach: Vec<EntityId>,
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

        let mut model = Self {
            records: read_records(ctx),
            project_paths: read_project_records(ctx),
            pending_title_generations: HashSet::new(),
        };
        model.schedule_auto_title_refresh_tick(ctx);
        model
    }

    pub fn records(&self) -> &[AgentSessionRecord] {
        &self.records
    }

    pub fn session(&self, session_id: &str) -> Option<&AgentSessionRecord> {
        self.records.iter().find(|record| record.id == session_id)
    }

    pub fn project_paths_from_sessions_for_environment<'a>(
        &'a self,
        environment_id: &'a str,
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.project_paths
            .iter()
            .filter(move |project| project.environment_id == environment_id)
            .map(|project| &project.project_path)
            .chain(
                self.records
                    .iter()
                    .filter(move |record| record.environment_id == environment_id)
                    .map(|record| &record.project_path),
            )
    }

    pub fn add_project_path(&mut self, project_path: PathBuf, ctx: &mut ModelContext<Self>) {
        let environment_id = SshRemoteModel::as_ref(ctx).active_environment_id();
        let now = now_ms();
        if let Some(project) = self.project_paths.iter_mut().find(|project| {
            project.environment_id == environment_id && project.project_path == project_path
        }) {
            project.updated_at_ms = now;
        } else {
            self.project_paths.push(AgentSessionProjectRecord {
                environment_id,
                project_path,
                updated_at_ms: now,
            });
        }
        self.persist_and_emit(ctx);
    }

    pub fn move_project_path(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        ctx: &mut ModelContext<Self>,
    ) {
        if old_path == new_path.as_path() {
            return;
        }

        let environment_id = SshRemoteModel::as_ref(ctx).active_environment_id();
        let mut changed = false;
        for project in self.project_paths.iter_mut().filter(|project| {
            project.environment_id == environment_id && project.project_path == old_path
        }) {
            project.project_path = new_path.clone();
            project.updated_at_ms = now_ms();
            changed = true;
        }
        for record in self.records.iter_mut().filter(|record| {
            record.environment_id == environment_id && record.project_path == old_path
        }) {
            record.project_path = new_path.clone();
            record.updated_at_ms = now_ms();
            changed = true;
        }

        if changed {
            self.persist_and_emit(ctx);
        }
    }

    pub fn delete_project_sessions(&mut self, project_path: &Path, ctx: &mut ModelContext<Self>) {
        let environment_id = SshRemoteModel::as_ref(ctx).active_environment_id();
        let original_len = self.records.len();
        let original_project_len = self.project_paths.len();
        self.project_paths.retain(|project| {
            !(project.environment_id == environment_id && project.project_path == project_path)
        });
        self.records.retain(|record| {
            !(record.environment_id == environment_id && record.project_path == project_path)
        });
        if self.records.len() != original_len || self.project_paths.len() != original_project_len {
            self.persist_and_emit(ctx);
        }
    }

    pub fn parent_or_self_session_id(&self, session_id: &str) -> Option<String> {
        let record = self.session(session_id)?;
        Some(
            record
                .parent_session_id
                .clone()
                .unwrap_or_else(|| record.id.clone()),
        )
    }

    pub fn has_active_children(&self, session_id: &str) -> bool {
        self.records.iter().any(|record| {
            record.parent_session_id.as_deref() == Some(session_id) && !record.is_archived()
        })
    }

    pub fn active_group_session_count(&self, session_id: &str) -> usize {
        let Some(parent_session_id) = self.parent_or_self_session_id(session_id) else {
            return 0;
        };

        self.records
            .iter()
            .filter(|record| {
                !record.is_archived()
                    && (record.id == parent_session_id
                        || record.parent_session_id.as_deref() == Some(parent_session_id.as_str()))
            })
            .count()
    }

    pub fn terminal_view_ids_for_disbanded_group(&self, parent_session_id: &str) -> Vec<EntityId> {
        let mut terminal_view_ids = Vec::new();
        let Some(parent_session) = self.session(parent_session_id) else {
            return terminal_view_ids;
        };

        for terminal_view_id in [
            parent_session.terminal_view_id,
            parent_session.group_terminal_view_id,
        ]
        .into_iter()
        .flatten()
        {
            if !terminal_view_ids.contains(&terminal_view_id) {
                terminal_view_ids.push(terminal_view_id);
            }
        }

        for record in self.records.iter().filter(|record| {
            record.parent_session_id.as_deref() == Some(parent_session_id) && !record.is_archived()
        }) {
            if let Some(terminal_view_id) = record.terminal_view_id {
                if !terminal_view_ids.contains(&terminal_view_id) {
                    terminal_view_ids.push(terminal_view_id);
                }
            }
        }

        terminal_view_ids
    }

    pub fn group_sessions_for_session(&self, session_id: &str) -> Vec<AgentSessionRecord> {
        let Some(parent_session_id) = self.parent_or_self_session_id(session_id) else {
            return Vec::new();
        };

        let Some(parent_session) = self
            .records
            .iter()
            .find(|record| record.id == parent_session_id && !record.is_archived())
            .cloned()
        else {
            return Vec::new();
        };

        let mut child_sessions = self
            .records
            .iter()
            .filter(|record| {
                record.parent_session_id.as_deref() == Some(parent_session_id.as_str())
                    && !record.is_archived()
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_sessions(&mut child_sessions);

        let mut sessions = vec![parent_session];
        sessions.extend(child_sessions);
        sessions
    }

    pub fn start_session(
        &mut self,
        project_path: PathBuf,
        agent: CLIAgent,
        ctx: &mut ModelContext<Self>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        let environment_id = SshRemoteModel::as_ref(ctx).active_environment_id();
        if !self.project_paths.iter().any(|project| {
            project.environment_id == environment_id && project.project_path == project_path
        }) {
            self.project_paths.push(AgentSessionProjectRecord {
                environment_id: environment_id.clone(),
                project_path: project_path.clone(),
                updated_at_ms: now,
            });
        }
        self.records.insert(
            0,
            AgentSessionRecord {
                id: id.clone(),
                environment_id,
                project_path,
                agent,
                title: new_session_title(agent),
                status: AgentSessionStatus::Starting,
                agent_session_id: None,
                parent_session_id: None,
                parent_agent_session_id: None,
                updated_at_ms: now,
                sort_order: now,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            },
        );
        self.trim_records();
        self.persist_and_emit(ctx);
        id
    }

    pub fn start_child_session(
        &mut self,
        parent_session_id: &str,
        ctx: &mut ModelContext<Self>,
    ) -> Option<String> {
        let parent_session_id = self.parent_or_self_session_id(parent_session_id)?;
        let parent = self.session(&parent_session_id)?.clone();
        if parent.is_archived() {
            return None;
        }

        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        self.records.insert(
            0,
            AgentSessionRecord {
                id: id.clone(),
                environment_id: parent.environment_id.clone(),
                project_path: parent.project_path.clone(),
                agent: parent.agent,
                title: format!("New {} child session", parent.agent.display_name()),
                status: AgentSessionStatus::Starting,
                agent_session_id: None,
                parent_session_id: Some(parent.id.clone()),
                parent_agent_session_id: parent.agent_session_id.clone(),
                updated_at_ms: now,
                sort_order: now,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            },
        );
        if let Some(parent_record) = self.record_mut(&parent.id) {
            parent_record.updated_at_ms = now;
        }
        self.trim_records();
        self.persist_and_emit(ctx);
        Some(id)
    }

    pub fn resolve_missing_agent_session_ids_for_project(
        &mut self,
        project_path: &Path,
        ctx: &mut ModelContext<Self>,
    ) {
        let mut changed = false;
        let session_ids = self
            .records
            .iter()
            .filter(|record| record.project_path == project_path)
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        for session_id in session_ids {
            changed |= self.resolve_missing_agent_session_id_inner(&session_id);
        }

        changed |= self.clear_duplicate_agent_session_ids_for_project(project_path);

        if changed {
            self.persist_and_emit(ctx);
        }

        self.sync_codex_child_sessions_for_project(project_path, ctx);
    }

    fn clear_duplicate_agent_session_ids_for_project(&mut self, project_path: &Path) -> bool {
        let mut codex_session_id_counts = HashMap::<(String, String), usize>::new();
        let mut codex_session_id_keeper = HashMap::<(String, String), (String, bool, i64)>::new();
        for record in self.records.iter().filter(|record| {
            record.project_path == project_path
                && record.agent == CLIAgent::Codex
                && !record.is_archived()
        }) {
            if let Some(session_id) = record.agent_session_id.as_ref() {
                let key = (record.environment_id.clone(), session_id.clone());
                *codex_session_id_counts.entry(key.clone()).or_default() += 1;

                let has_live_terminal =
                    record.terminal_view_id.is_some() || record.group_terminal_view_id.is_some();
                let candidate = (record.id.clone(), has_live_terminal, record.updated_at_ms);
                match codex_session_id_keeper.get(&key) {
                    Some((_, kept_has_live_terminal, _))
                        if !has_live_terminal && *kept_has_live_terminal =>
                    {
                        // Prefer the currently attached terminal; it is the least surprising
                        // record to keep when cleaning up old shared resume ids.
                    }
                    Some((_, kept_has_live_terminal, kept_updated_at_ms))
                        if has_live_terminal == *kept_has_live_terminal
                            && record.updated_at_ms <= *kept_updated_at_ms => {}
                    _ => {
                        codex_session_id_keeper.insert(key, candidate);
                    }
                }
            }
        }

        let mut changed = false;
        for record in self.records.iter_mut().filter(|record| {
            record.project_path == project_path
                && record.agent == CLIAgent::Codex
                && !record.is_archived()
        }) {
            let Some(session_id) = record.agent_session_id.as_ref() else {
                continue;
            };
            let key = (record.environment_id.clone(), session_id.clone());
            if codex_session_id_counts.get(&key).copied().unwrap_or(0) <= 1 {
                continue;
            }

            let should_keep = codex_session_id_keeper
                .get(&key)
                .is_some_and(|(record_id, _, _)| record_id == &record.id);
            if !should_keep {
                record.agent_session_id = None;
                changed = true;
            }
        }

        changed
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

    pub fn attach_group_terminal(
        &mut self,
        session_id: &str,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(parent_session_id) = self.parent_or_self_session_id(session_id) else {
            return;
        };
        let Some(record) = self.record_mut(&parent_session_id) else {
            return;
        };
        record.group_terminal_view_id = Some(terminal_view_id);
        record.updated_at_ms = now_ms();
        self.persist_and_emit(ctx);
    }

    pub fn detach_terminal_views(
        &mut self,
        terminal_view_ids: &[EntityId],
        ctx: &mut ModelContext<Self>,
    ) {
        if terminal_view_ids.is_empty() {
            return;
        }

        let mut changed = false;
        for record in &mut self.records {
            if record
                .terminal_view_id
                .is_some_and(|terminal_view_id| terminal_view_ids.contains(&terminal_view_id))
            {
                record.terminal_view_id = None;
                record.updated_at_ms = now_ms();
                changed = true;
            }
            if record
                .group_terminal_view_id
                .is_some_and(|terminal_view_id| terminal_view_ids.contains(&terminal_view_id))
            {
                record.group_terminal_view_id = None;
                record.updated_at_ms = now_ms();
                changed = true;
            }
        }

        if changed {
            self.persist_and_emit(ctx);
        }
    }

    pub fn resolve_missing_agent_session_id(
        &mut self,
        session_id: &str,
        ctx: &mut ModelContext<Self>,
    ) -> Option<String> {
        let changed = self.resolve_missing_agent_session_id_inner(session_id);
        if changed {
            self.persist_and_emit(ctx);
        }
        self.session(session_id)?.agent_session_id.clone()
    }

    pub fn sync_agent_session_id_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        session_context: Option<&CLIAgentSessionContext>,
        ctx: &mut ModelContext<Self>,
    ) -> Option<String> {
        let record_id = self
            .records
            .iter()
            .find(|record| {
                record.terminal_view_id == Some(terminal_view_id)
                    || record.group_terminal_view_id == Some(terminal_view_id)
            })?
            .id
            .clone();

        let mut changed = false;
        if let Some(session_id) = session_context
            .and_then(|context| context.session_id.as_deref())
            .map(str::trim)
            .filter(|session_id| !session_id.is_empty())
        {
            let session_id = session_id.to_owned();
            if let Some(record) = self.record_mut(&record_id) {
                if record.agent_session_id.as_deref() != Some(session_id.as_str()) {
                    record.agent_session_id = Some(session_id);
                    record.updated_at_ms = now_ms();
                    changed = true;
                }
            }
        } else {
            changed |= self.resolve_missing_agent_session_id_inner(&record_id);
        }

        if changed {
            self.persist_and_emit(ctx);
        }

        self.session(&record_id)?.agent_session_id.clone()
    }

    pub fn latest_agent_session_id_for_project(
        agent: CLIAgent,
        project_path: &Path,
    ) -> Option<String> {
        match agent {
            CLIAgent::Codex => latest_codex_session_id_for_project(project_path),
            _ => None,
        }
    }

    fn resolve_missing_agent_session_id_inner(&mut self, session_id: &str) -> bool {
        let Some(record) = self.session(session_id) else {
            return false;
        };
        if record.agent_session_id.is_some() || record.agent != CLIAgent::Codex {
            return false;
        }
        let environment_id = record.environment_id.clone();

        let Some(agent_session_id) = latest_codex_session_id_for_project(&record.project_path)
        else {
            return false;
        };
        if self.records.iter().any(|record| {
            record.id != session_id
                && record.agent == CLIAgent::Codex
                && record.environment_id == environment_id
                && record.agent_session_id.as_deref() == Some(agent_session_id.as_str())
        }) {
            return false;
        }

        let now = now_ms();
        let Some(record) = self.record_mut(session_id) else {
            return false;
        };
        record.agent_session_id = Some(agent_session_id);
        record.updated_at_ms = now;
        true
    }

    pub fn sync_codex_child_sessions_for_project(
        &mut self,
        project_path: &Path,
        ctx: &mut ModelContext<Self>,
    ) {
        let parent_sessions_by_agent_id = self
            .records
            .iter()
            .filter(|record| {
                record.project_path == project_path
                    && record.agent == CLIAgent::Codex
                    && !record.is_archived()
            })
            .filter_map(|record| {
                record.agent_session_id.as_ref().map(|agent_session_id| {
                    (
                        (record.environment_id.clone(), agent_session_id.clone()),
                        (record.id.clone(), record.environment_id.clone()),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        if parent_sessions_by_agent_id.is_empty() {
            return;
        }
        let parent_agent_session_ids = parent_sessions_by_agent_id
            .keys()
            .map(|(_, agent_session_id)| agent_session_id.clone())
            .collect();

        let mut known_agent_session_ids = self
            .records
            .iter()
            .filter_map(|record| {
                record.agent_session_id.as_ref().map(|agent_session_id| {
                    (record.environment_id.clone(), agent_session_id.clone())
                })
            })
            .collect::<BTreeSet<_>>();
        let mut changed = false;

        for child in codex_child_sessions_for_project(project_path, &parent_agent_session_ids) {
            let Some((parent_session_id, environment_id)) = parent_sessions_by_agent_id
                .iter()
                .find_map(|((environment_id, agent_session_id), parent)| {
                    (agent_session_id == &child.parent_agent_session_id)
                        .then(|| (parent.0.clone(), environment_id.clone()))
                })
            else {
                continue;
            };
            if parent_sessions_by_agent_id.contains_key(&(environment_id.clone(), child.id.clone()))
                || !known_agent_session_ids.insert((environment_id.clone(), child.id.clone()))
            {
                continue;
            }

            self.records.insert(
                0,
                AgentSessionRecord {
                    id: Uuid::new_v4().to_string(),
                    environment_id,
                    project_path: project_path.to_path_buf(),
                    agent: CLIAgent::Codex,
                    title: truncate_title(child.title),
                    status: AgentSessionStatus::Success,
                    agent_session_id: Some(child.id),
                    parent_session_id: Some(parent_session_id),
                    parent_agent_session_id: Some(child.parent_agent_session_id),
                    updated_at_ms: child.modified_at_ms,
                    sort_order: child.modified_at_ms,
                    is_pinned: false,
                    archived_at_ms: None,
                    title_overridden: false,
                    auto_title_fingerprint: None,
                    auto_title_summarized_at_ms: None,
                    auto_title_source_chars: 0,
                    hosted_transcript: None,
                    hosted_transcript_updated_at_ms: None,
                    terminal_view_id: None,
                    group_terminal_view_id: None,
                },
            );
            changed = true;
        }

        if changed {
            self.trim_records();
            self.persist_and_emit(ctx);
        }
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
        let is_group_root = self.has_active_children(session_id);
        let target_ids = if is_group_root {
            self.records
                .iter()
                .filter(|record| {
                    record.id == session_id
                        || record.parent_session_id.as_deref() == Some(session_id)
                })
                .map(|record| record.id.clone())
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::from([session_id.to_owned()])
        };
        let Some(record) = self.records.iter().find(|record| record.id == session_id) else {
            return;
        };
        let archived_at_ms = if record.archived_at_ms.is_some() {
            None
        } else {
            Some(now_ms())
        };
        for record in self
            .records
            .iter_mut()
            .filter(|record| target_ids.contains(&record.id))
        {
            record.archived_at_ms = archived_at_ms;
            record.updated_at_ms = now_ms();
        }
        self.persist_and_emit(ctx);
    }

    pub fn delete_session(&mut self, session_id: &str, ctx: &mut ModelContext<Self>) {
        let original_len = self.records.len();
        self.records.retain(|record| {
            record.id != session_id && record.parent_session_id.as_deref() != Some(session_id)
        });
        if self.records.len() != original_len {
            self.persist_and_emit(ctx);
        }
    }

    pub fn save_session_id_record(
        &mut self,
        record_id: Option<&str>,
        agent: CLIAgent,
        project_path: PathBuf,
        agent_session_id: String,
        title: String,
        ctx: &mut ModelContext<Self>,
    ) -> Result<String, String> {
        let agent_session_id = agent_session_id.trim().to_owned();
        if agent_session_id.is_empty() {
            return Err("Session ID is required".to_owned());
        }
        if project_path.as_os_str().is_empty() {
            return Err("Project path is required".to_owned());
        }

        let title = title.trim().to_owned();
        let title = if title.is_empty() {
            new_session_title(agent)
        } else {
            title
        };
        let now = now_ms();

        if let Some(record_id) = record_id {
            let Some(record) = self.record_mut(record_id) else {
                return Err("Session record not found".to_owned());
            };
            record.agent = agent;
            record.project_path = project_path;
            record.title = title;
            record.agent_session_id = Some(agent_session_id);
            record.updated_at_ms = now;
            self.persist_and_emit(ctx);
            return Ok(record_id.to_owned());
        }

        if let Some(record) = self.records.iter_mut().find(|record| {
            record.agent == agent
                && record.environment_id == default_agent_session_environment_id()
                && record.project_path == project_path
                && record.agent_session_id.as_deref() == Some(agent_session_id.as_str())
        }) {
            record.title = title;
            record.updated_at_ms = now;
            let id = record.id.clone();
            self.persist_and_emit(ctx);
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        self.records.push(AgentSessionRecord {
            id: id.clone(),
            environment_id: default_agent_session_environment_id(),
            project_path,
            agent,
            title,
            status: AgentSessionStatus::Unknown,
            agent_session_id: Some(agent_session_id),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: now,
            sort_order: now,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: true,
            auto_title_fingerprint: None,
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: None,
            hosted_transcript_updated_at_ms: None,
            terminal_view_id: None,
            group_terminal_view_id: None,
        });
        sort_sessions(&mut self.records);
        self.trim_records();
        self.persist_and_emit(ctx);
        Ok(id)
    }

    pub fn disband_group(&mut self, parent_session_id: &str, ctx: &mut ModelContext<Self>) {
        if self.session(parent_session_id).is_none() {
            return;
        }

        let now = now_ms();
        let mut changed = false;
        let child_ids = self
            .records
            .iter()
            .filter(|record| record.parent_session_id.as_deref() == Some(parent_session_id))
            .map(|record| record.id.clone())
            .collect::<BTreeSet<_>>();

        if let Some(parent) = self.record_mut(parent_session_id) {
            if parent.group_terminal_view_id.take().is_some() {
                changed = true;
            }
            if parent.terminal_view_id.take().is_some() {
                changed = true;
            }
            parent.updated_at_ms = now;
        }

        for record in self
            .records
            .iter_mut()
            .filter(|record| child_ids.contains(&record.id))
        {
            record.parent_session_id = None;
            record.parent_agent_session_id = None;
            record.terminal_view_id = None;
            record.updated_at_ms = now;
            record.sort_order = now;
            changed = true;
        }

        if changed {
            self.persist_and_emit(ctx);
        }
    }

    pub fn move_session(
        &mut self,
        source_id: &str,
        target_id: &str,
        placement: AgentSessionMovePlacement,
        ctx: &mut ModelContext<Self>,
    ) -> Option<AgentSessionMoveOutcome> {
        if source_id == target_id {
            return None;
        }

        let source = self.session(source_id)?.clone();
        let target = self.session(target_id)?.clone();
        if source.is_archived() || target.is_archived() {
            return None;
        }

        let previous_parent_id = self.parent_or_self_session_id(source_id)?;
        let previous_parent_was_group = self.has_active_children(&previous_parent_id)
            || self
                .session(&previous_parent_id)
                .is_some_and(|record| record.group_terminal_view_id.is_some());
        let target_parent_id = match placement {
            AgentSessionMovePlacement::IntoGroup => {
                Some(self.parent_or_self_session_id(target_id)?)
            }
            AgentSessionMovePlacement::Before | AgentSessionMovePlacement::After => {
                target.parent_session_id.clone()
            }
        };

        if target_parent_id.as_deref() == Some(source_id) {
            return None;
        }

        let moving_group_root =
            source.parent_session_id.is_none() && self.has_active_children(source_id);
        let mut moved_ids = vec![source_id.to_owned()];
        if moving_group_root && target_parent_id.is_some() {
            let mut child_records = self
                .records
                .iter()
                .filter(|record| {
                    record.parent_session_id.as_deref() == Some(source_id) && !record.is_archived()
                })
                .cloned()
                .collect::<Vec<_>>();
            sort_sessions(&mut child_records);
            moved_ids.extend(child_records.into_iter().map(|record| record.id));
        }

        if moved_ids.iter().any(|id| id == target_id) {
            return None;
        }

        let parent_changed = source.parent_session_id != target_parent_id;
        let mut terminal_view_ids_to_detach = Vec::new();
        if parent_changed {
            for record in self
                .records
                .iter()
                .filter(|record| moved_ids.iter().any(|id| id == &record.id))
            {
                for terminal_view_id in [record.terminal_view_id, record.group_terminal_view_id]
                    .into_iter()
                    .flatten()
                {
                    if !terminal_view_ids_to_detach.contains(&terminal_view_id) {
                        terminal_view_ids_to_detach.push(terminal_view_id);
                    }
                }
            }
        }

        let target_project_path = if let Some(parent_id) = target_parent_id.as_deref() {
            self.session(parent_id)?.project_path.clone()
        } else {
            target.project_path.clone()
        };
        let target_parent_agent_session_id = target_parent_id
            .as_deref()
            .and_then(|parent_id| self.session(parent_id))
            .and_then(|record| record.agent_session_id.clone());
        let now = now_ms();

        for moved_id in &moved_ids {
            let Some(record) = self.record_mut(moved_id) else {
                continue;
            };
            record.parent_session_id = target_parent_id.clone();
            record.parent_agent_session_id = target_parent_agent_session_id.clone();
            record.project_path = target_project_path.clone();
            record.updated_at_ms = now;
            if moved_id == source_id && moving_group_root && target_parent_id.is_some() {
                record.group_terminal_view_id = None;
            }
        }

        self.reorder_moved_sessions(
            &moved_ids,
            target_parent_id.as_deref(),
            target_id,
            placement,
            &target_project_path,
            now,
        );

        let new_parent_id = self.parent_or_self_session_id(source_id);
        let singleton_group_id = if previous_parent_was_group
            && new_parent_id.as_deref() != Some(previous_parent_id.as_str())
            && self.active_group_session_count(&previous_parent_id) <= 1
        {
            Some(previous_parent_id)
        } else {
            None
        };

        self.persist_and_emit(ctx);
        Some(AgentSessionMoveOutcome {
            expanded_group_id: target_parent_id,
            singleton_group_id,
            terminal_view_ids_to_detach,
        })
    }

    pub fn terminal_view_ids_for_deleted_session(&self, session_id: &str) -> Vec<EntityId> {
        let mut terminal_view_ids = Vec::new();
        for record in self.records.iter().filter(|record| {
            record.id == session_id || record.parent_session_id.as_deref() == Some(session_id)
        }) {
            for terminal_view_id in [record.terminal_view_id, record.group_terminal_view_id]
                .into_iter()
                .flatten()
            {
                if !terminal_view_ids.contains(&terminal_view_id) {
                    terminal_view_ids.push(terminal_view_id);
                }
            }
        }
        terminal_view_ids
    }

    pub fn update_hosted_transcript_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        transcript: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let transcript = normalize_hosted_transcript(transcript);
        let Some(record_index) = self.records.iter().position(|record| {
            record.terminal_view_id == Some(terminal_view_id)
                || record.group_terminal_view_id == Some(terminal_view_id)
        }) else {
            return;
        };

        if self.records[record_index].hosted_transcript == transcript {
            return;
        }

        let session_id = self.records[record_index].id.clone();
        let project_path = self.records[record_index].project_path.clone();
        let record = &mut self.records[record_index];
        record.hosted_transcript = transcript;
        record.hosted_transcript_updated_at_ms = Some(now_ms());
        record.updated_at_ms = now_ms();

        let mut changed = true;
        changed |= self.resolve_missing_agent_session_id_inner(&session_id);
        changed |= self.clear_duplicate_agent_session_ids_for_project(&project_path);
        changed |= self.maybe_update_auto_title_for_terminal(terminal_view_id, ctx);

        if changed {
            self.persist_and_emit(ctx);
        }
    }

    fn handle_cli_agent_sessions_event(
        &mut self,
        event: &CLIAgentSessionsModelEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let terminal_view_id = event.terminal_view_id();
        let mut changed = match event {
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
            } => {
                let sidebar_status = AgentSessionStatus::from_cli_status(status);
                let changed = self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    record.status = sidebar_status;
                    if !record.title_overridden && record.auto_title_fingerprint.is_none() {
                        if let Some(title) = first_prompt_session_title(session_context) {
                            record.title = truncate_title(title);
                        }
                    }
                    if let Some(session_id) = &session_context.session_id {
                        record.agent_session_id = Some(session_id.clone());
                    }
                    record.capture_session_context(session_context);
                });

                changed
                    || self.insert_or_attach_cli_session_record(
                        terminal_view_id,
                        *agent,
                        session_context,
                        sidebar_status,
                        ctx,
                    )
            }
            CLIAgentSessionsModelEvent::SessionUpdated { agent, .. } => {
                let session = CLIAgentSessionsModel::as_ref(ctx)
                    .session(terminal_view_id)
                    .cloned();
                let changed = self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    if let Some(session) = &session {
                        let sidebar_status = AgentSessionStatus::from_cli_status(&session.status);
                        record.status = sidebar_status;
                        if !record.title_overridden && record.auto_title_fingerprint.is_none() {
                            if let Some(title) =
                                first_prompt_session_title(&session.session_context)
                            {
                                record.title = truncate_title(title);
                            }
                        }
                        if let Some(session_id) = &session.session_context.session_id {
                            record.agent_session_id = Some(session_id.clone());
                        }
                        record.capture_session_context(&session.session_context);
                    }
                });

                if changed {
                    true
                } else if let Some(session) = session {
                    self.insert_or_attach_cli_session_record(
                        terminal_view_id,
                        *agent,
                        &session.session_context,
                        AgentSessionStatus::from_cli_status(&session.status),
                        ctx,
                    )
                } else {
                    false
                }
            }
            CLIAgentSessionsModelEvent::InputSessionChanged { .. } => false,
            CLIAgentSessionsModelEvent::Ended { agent, .. } => {
                self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    record.status = AgentSessionStatus::Success;
                })
            }
        };

        if matches!(
            event,
            CLIAgentSessionsModelEvent::StatusChanged { .. }
                | CLIAgentSessionsModelEvent::SessionUpdated { .. }
        ) {
            changed |= self.maybe_update_auto_title_for_terminal(terminal_view_id, ctx);
        }

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

    fn insert_or_attach_cli_session_record(
        &mut self,
        terminal_view_id: EntityId,
        agent: CLIAgent,
        session_context: &CLIAgentSessionContext,
        status: AgentSessionStatus,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self
            .records
            .iter()
            .any(|record| record.terminal_view_id == Some(terminal_view_id))
        {
            return false;
        }

        let environment_id = SshRemoteModel::as_ref(ctx).active_environment_id();
        if let Some(agent_session_id) = session_context.session_id.as_deref() {
            if let Some(record) = self.records.iter_mut().find(|record| {
                record.agent == agent
                    && record.environment_id == environment_id
                    && record.agent_session_id.as_deref() == Some(agent_session_id)
            }) {
                record.terminal_view_id = Some(terminal_view_id);
                record.status = status;
                record.updated_at_ms = now_ms();
                record.capture_session_context(session_context);
                return true;
            }
        }

        let Some(project_path) = project_path_from_session_context(session_context) else {
            return false;
        };

        let now = now_ms();
        let mut record = AgentSessionRecord {
            id: Uuid::new_v4().to_string(),
            environment_id,
            project_path,
            agent,
            title: first_prompt_session_title(session_context)
                .map(truncate_title)
                .unwrap_or_else(|| new_session_title(agent)),
            status,
            agent_session_id: session_context.session_id.clone(),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: now,
            sort_order: now,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            auto_title_fingerprint: None,
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: None,
            hosted_transcript_updated_at_ms: None,
            terminal_view_id: Some(terminal_view_id),
            group_terminal_view_id: None,
        };
        record.capture_session_context(session_context);

        self.records.insert(0, record);
        self.trim_records();
        true
    }

    fn schedule_auto_title_refresh_tick(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.spawn(
            async move { Timer::after(AUTO_TITLE_REFRESH_SCAN_INTERVAL).await },
            |model, _, ctx| {
                let changed = model.refresh_due_auto_titles(ctx);
                if changed {
                    model.persist_and_emit(ctx);
                }
                model.schedule_auto_title_refresh_tick(ctx);
            },
        );
    }

    fn refresh_due_auto_titles(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        let mut terminal_view_ids = Vec::new();
        for record in &self.records {
            for terminal_view_id in [record.terminal_view_id, record.group_terminal_view_id]
                .into_iter()
                .flatten()
            {
                if !terminal_view_ids.contains(&terminal_view_id) {
                    terminal_view_ids.push(terminal_view_id);
                }
            }
        }

        let mut changed = false;
        for terminal_view_id in terminal_view_ids {
            changed |= self.maybe_update_auto_title_for_terminal(terminal_view_id, ctx);
        }
        changed
    }

    fn maybe_update_auto_title_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let session_context = CLIAgentSessionsModel::as_ref(ctx)
            .session(terminal_view_id)
            .map(|session| session.session_context.clone());
        let Some((session_id, request)) = self
            .records
            .iter()
            .find(|record| {
                record.terminal_view_id == Some(terminal_view_id)
                    || record.group_terminal_view_id == Some(terminal_view_id)
            })
            .and_then(|record| {
                agent_session_title_request(record, session_context.as_ref())
                    .map(|request| (record.id.clone(), request))
            })
        else {
            return false;
        };

        let Some(record) = self.session(&session_id) else {
            return false;
        };
        if record.title_overridden {
            return false;
        }

        let now = now_ms();
        let Some(action) = auto_title_action(record, &request, now) else {
            return false;
        };

        if matches!(action, AutoTitleAction::Refresh) {
            let pending_prefix = format!("{session_id}:");
            if self
                .pending_title_generations
                .iter()
                .any(|key| key.starts_with(&pending_prefix))
            {
                return false;
            }
        }

        let Some(record) = self.record_mut(&session_id) else {
            return false;
        };
        record.auto_title_fingerprint = Some(request.fingerprint);
        record.auto_title_summarized_at_ms = Some(now);
        record.auto_title_source_chars = request.source_chars;
        record.updated_at_ms = now;

        if matches!(action, AutoTitleAction::FirstPrompt) {
            let Some(first_prompt_title) = request.first_prompt_title.clone() else {
                return true;
            };
            if record.title != first_prompt_title {
                record.title = first_prompt_title;
            }
            return true;
        }

        let pending_key = format!("{}:{}", session_id, request.fingerprint);
        if self.pending_title_generations.insert(pending_key.clone()) {
            self.spawn_auto_title_generation(session_id, request, pending_key, ctx);
        }

        true
    }

    fn spawn_auto_title_generation(
        &mut self,
        session_id: String,
        request: AgentSessionTitleRequest,
        pending_key: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let fingerprint = request.fingerprint;
        let agent = request.agent;
        let project_path = request.project_path.clone();
        let prompt = request.prompt.clone();
        let fallback_prompt = request.prompt;
        let fallback_title = request.fallback_title.clone();
        let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();
        ctx.spawn(
            async move {
                if let Some(title) =
                    generate_title_with_matching_agent(agent, &project_path, &prompt).await
                {
                    return Some(title);
                }

                match ai_client
                    .generate_dialogue_answer(Vec::new(), fallback_prompt, None)
                    .await
                {
                    Ok(GenerateDialogueResult::Success { answer, .. }) => {
                        sanitize_generated_session_title(&answer)
                    }
                    Ok(GenerateDialogueResult::Failure { .. }) => Some(fallback_title),
                    Err(err) => {
                        log::warn!("Failed to generate agent session title: {err:?}");
                        Some(fallback_title)
                    }
                }
            },
            move |model, generated_title, ctx| {
                model.pending_title_generations.remove(&pending_key);
                let Some(generated_title) = generated_title else {
                    return;
                };
                let Some(record) = model.record_mut(&session_id) else {
                    return;
                };
                if record.title_overridden
                    || record.auto_title_fingerprint != Some(fingerprint)
                    || record.title == generated_title
                {
                    return;
                }

                record.title = generated_title;
                record.updated_at_ms = now_ms();
                model.persist_and_emit(ctx);
            },
        );
    }

    fn record_mut(&mut self, session_id: &str) -> Option<&mut AgentSessionRecord> {
        self.records
            .iter_mut()
            .find(|record| record.id == session_id)
    }

    fn reorder_moved_sessions(
        &mut self,
        moved_ids: &[String],
        parent_session_id: Option<&str>,
        target_id: &str,
        placement: AgentSessionMovePlacement,
        project_path: &Path,
        now: i64,
    ) {
        let active_ids = self
            .records
            .iter()
            .filter(|record| !record.is_archived())
            .map(|record| record.id.clone())
            .collect::<BTreeSet<_>>();
        let mut sibling_records = self
            .records
            .iter()
            .filter(|record| {
                if record.is_archived() || record.project_path != project_path {
                    return false;
                }
                match parent_session_id {
                    Some(parent_id) => record.parent_session_id.as_deref() == Some(parent_id),
                    None => record
                        .parent_session_id
                        .as_deref()
                        .is_none_or(|parent_id| !active_ids.contains(parent_id)),
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_sessions(&mut sibling_records);

        let moved_id_set = moved_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut ordered_ids = sibling_records
            .into_iter()
            .map(|record| record.id)
            .filter(|id| !moved_id_set.contains(id))
            .collect::<Vec<_>>();

        let insert_index = match placement {
            AgentSessionMovePlacement::IntoGroup => 0,
            AgentSessionMovePlacement::Before | AgentSessionMovePlacement::After => {
                let target_index = ordered_ids
                    .iter()
                    .position(|id| id == target_id)
                    .unwrap_or(ordered_ids.len());
                if matches!(placement, AgentSessionMovePlacement::After) {
                    target_index.saturating_add(1).min(ordered_ids.len())
                } else {
                    target_index
                }
            }
        };
        for (offset, moved_id) in moved_ids.iter().enumerate() {
            ordered_ids.insert(insert_index + offset, moved_id.clone());
        }

        let base_order = now.saturating_add((ordered_ids.len() as i64 + 1) * 1_000);
        for (index, id) in ordered_ids.into_iter().enumerate() {
            if let Some(record) = self.record_mut(&id) {
                record.sort_order = base_order - (index as i64 * 1_000);
                record.updated_at_ms = now;
            }
        }
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
        if let Ok(serialized) = serde_json::to_string(&self.project_paths) {
            if let Err(err) = ctx
                .private_user_preferences()
                .write_value(AGENT_SESSION_PROJECTS_PREF_KEY, serialized)
            {
                log::error!("Failed to persist agent session projects: {err}");
            }
        }
        ctx.emit(AgentSessionsModelEvent);
    }
}

pub struct AgentSessionsView {
    window_id: WindowId,
    scroll_state: ClippedScrollStateHandle,
    row_mouse_states: RefCell<HashMap<String, MouseStateHandle>>,
    row_draggable_states: RefCell<HashMap<String, DraggableState>>,
    expanded_group_ids: RefCell<BTreeSet<String>>,
    collapsed_project_keys: RefCell<BTreeSet<String>>,
    project_order: RefCell<Vec<String>>,
    active_drag_session_id: Option<String>,
    active_drag_project_key: Option<String>,
    hovered_drop_target: Option<AgentSessionDropTarget>,
    hovered_project_drop_target: Option<AgentProjectDropTarget>,
    open_session_actions_id: Option<String>,
    open_project_actions_key: Option<String>,
    pending_singleton_group_prompt: Option<SingletonGroupPrompt>,
    pending_delete_confirmation: Option<PendingDeleteConfirmation>,
    rename_editor: ViewHandle<EditorView>,
    renaming_session_id: Option<String>,
    projects_header_mouse_state: MouseStateHandle,
    add_project_mouse_state: MouseStateHandle,
    empty_state_mouse_state: MouseStateHandle,
    singleton_prompt_remember_mouse_state: MouseStateHandle,
    singleton_prompt_keep_mouse_state: MouseStateHandle,
    singleton_prompt_disband_mouse_state: MouseStateHandle,
    singleton_prompt_close_mouse_state: MouseStateHandle,
    delete_prompt_cancel_mouse_state: MouseStateHandle,
    delete_prompt_delete_mouse_state: MouseStateHandle,
    delete_prompt_close_mouse_state: MouseStateHandle,
    ssh_remote_loading_tick: u8,
    ssh_remote_loading_tick_scheduled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSessionDropTargetData {
    session_id: String,
}

impl DropTargetData for AgentSessionDropTargetData {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentProjectDropTarget {
    target_project_key: String,
    placement: AgentProjectMovePlacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentProjectDropTargetData {
    project_key: String,
}

impl DropTargetData for AgentProjectDropTargetData {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSessionDropTarget {
    target_session_id: String,
    placement: AgentSessionMovePlacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SingletonGroupPrompt {
    parent_session_id: String,
    remember_choice: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingDeleteConfirmation {
    Session { session_id: String, title: String },
    Project { project_path: PathBuf, name: String },
}

impl AgentSessionsView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&AgentSessionsModel::handle(ctx), |_me, _, _event, ctx| {
            ctx.notify();
        });
        ctx.subscribe_to_model(&ActiveSession::handle(ctx), |_me, _, _event, ctx| {
            ctx.notify();
        });
        ctx.subscribe_to_model(&SshRemoteModel::handle(ctx), |me, _, _event, ctx| {
            if me.is_ssh_remote_environment_connecting(ctx) {
                me.schedule_ssh_remote_loading_tick(ctx);
            }
            ctx.notify();
        });
        ctx.subscribe_to_model(
            &AgentReasoningEffortModel::handle(ctx),
            |_me, _, _event, ctx| {
                ctx.notify();
            },
        );
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
            window_id: ctx.window_id(),
            scroll_state: ClippedScrollStateHandle::default(),
            row_mouse_states: RefCell::new(HashMap::new()),
            row_draggable_states: RefCell::new(HashMap::new()),
            expanded_group_ids: RefCell::new(BTreeSet::new()),
            collapsed_project_keys: RefCell::new(read_collapsed_project_keys(ctx)),
            project_order: RefCell::new(read_project_order(ctx)),
            active_drag_session_id: None,
            active_drag_project_key: None,
            hovered_drop_target: None,
            hovered_project_drop_target: None,
            open_session_actions_id: None,
            open_project_actions_key: None,
            pending_singleton_group_prompt: None,
            pending_delete_confirmation: None,
            rename_editor,
            renaming_session_id: None,
            projects_header_mouse_state: MouseStateHandle::default(),
            add_project_mouse_state: MouseStateHandle::default(),
            empty_state_mouse_state: MouseStateHandle::default(),
            singleton_prompt_remember_mouse_state: MouseStateHandle::default(),
            singleton_prompt_keep_mouse_state: MouseStateHandle::default(),
            singleton_prompt_disband_mouse_state: MouseStateHandle::default(),
            singleton_prompt_close_mouse_state: MouseStateHandle::default(),
            delete_prompt_cancel_mouse_state: MouseStateHandle::default(),
            delete_prompt_delete_mouse_state: MouseStateHandle::default(),
            delete_prompt_close_mouse_state: MouseStateHandle::default(),
            ssh_remote_loading_tick: 0,
            ssh_remote_loading_tick_scheduled: false,
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

    fn current_environment_id(app: &AppContext) -> String {
        SshRemoteModel::as_ref(app).active_environment_id()
    }

    fn connecting_ssh_remote_host(app: &AppContext) -> Option<SshRemoteHost> {
        let model = SshRemoteModel::as_ref(app);
        let host = model.pending_active_host()?;
        matches!(
            model.connection_status(&host.id),
            SshRemoteConnectionStatus::Connecting
        )
        .then(|| host.clone())
    }

    fn is_ssh_remote_environment_connecting(&self, app: &AppContext) -> bool {
        Self::connecting_ssh_remote_host(app).is_some()
    }

    fn schedule_ssh_remote_loading_tick(&mut self, ctx: &mut ViewContext<Self>) {
        if self.ssh_remote_loading_tick_scheduled {
            return;
        }
        self.ssh_remote_loading_tick_scheduled = true;
        ctx.spawn(
            async move { Timer::after(SSH_REMOTE_LOADING_TICK).await },
            |view, _, ctx| {
                view.ssh_remote_loading_tick_scheduled = false;
                if view.is_ssh_remote_environment_connecting(ctx) {
                    view.ssh_remote_loading_tick = view.ssh_remote_loading_tick.wrapping_add(1);
                    view.schedule_ssh_remote_loading_tick(ctx);
                }
                ctx.notify();
            },
        );
    }

    fn active_terminal_view_id(&self, app: &AppContext) -> Option<EntityId> {
        ActiveSession::as_ref(app).terminal_view_id(self.window_id)
    }

    fn session_matches_terminal(
        session: &AgentSessionRecord,
        children: &[AgentSessionRecord],
        terminal_view_id: Option<EntityId>,
    ) -> bool {
        let Some(terminal_view_id) = terminal_view_id else {
            return false;
        };
        session.terminal_view_id == Some(terminal_view_id)
            || session.group_terminal_view_id == Some(terminal_view_id)
            || children
                .iter()
                .any(|child| child.terminal_view_id == Some(terminal_view_id))
    }

    fn project_paths(&self, app: &AppContext) -> Vec<PathBuf> {
        let environment_id = Self::current_environment_id(app);
        let mut paths = BTreeSet::new();
        if environment_id == SSH_REMOTE_LOCAL_ENVIRONMENT_ID {
            for project in ProjectManagementModel::as_ref(app).all_projects() {
                paths.insert(PathBuf::from(project.path.clone()));
            }
        }
        for path in AgentSessionsModel::as_ref(app)
            .project_paths_from_sessions_for_environment(&environment_id)
        {
            paths.insert(path.clone());
        }
        let mut paths = paths.into_iter().collect::<Vec<_>>();
        self.sync_project_order_for_paths(&paths);
        let order = self.project_order.borrow();
        let order_index = order
            .iter()
            .enumerate()
            .map(|(index, key)| (key.as_str(), index))
            .collect::<HashMap<_, _>>();
        paths.sort_by(|a, b| {
            let a_key = project_order_key(a);
            let b_key = project_order_key(b);
            order_index
                .get(a_key.as_str())
                .copied()
                .unwrap_or(usize::MAX)
                .cmp(
                    &order_index
                        .get(b_key.as_str())
                        .copied()
                        .unwrap_or(usize::MAX),
                )
                .then_with(|| friendly_path(a).cmp(&friendly_path(b)))
        });
        paths
    }

    fn sync_project_order_for_paths(&self, paths: &[PathBuf]) {
        let project_keys = paths
            .iter()
            .map(|path| project_order_key(path))
            .collect::<BTreeSet<_>>();
        let mut order = self.project_order.borrow_mut();
        order.retain(|key| project_keys.contains(key));
        for key in project_keys {
            if !order.contains(&key) {
                order.push(key);
            }
        }
    }

    fn project_drop_target_for_drag(
        &self,
        source_project_key: &str,
        target_project_key: &str,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) -> Option<AgentProjectDropTarget> {
        if source_project_key == target_project_key {
            return None;
        }

        let target_position =
            ctx.element_position_by_id(agent_project_position_id(target_project_key))?;
        let placement = if drag_position.center().y() < target_position.center().y() {
            AgentProjectMovePlacement::Before
        } else {
            AgentProjectMovePlacement::After
        };

        Some(AgentProjectDropTarget {
            target_project_key: target_project_key.to_owned(),
            placement,
        })
    }

    fn candidate_project_keys_for_drag(
        &self,
        source_project_key: &str,
        drag_position: RectF,
        framework_target_project_key: Option<&str>,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<String> {
        let drag_center = drag_position.center();
        let mut candidates = self
            .project_paths(ctx)
            .into_iter()
            .map(|path| project_order_key(&path))
            .filter(|project_key| project_key != source_project_key)
            .filter_map(|project_key| {
                let bounds = ctx.element_position_by_id(agent_project_position_id(&project_key))?;
                let center_in_y =
                    drag_center.y() >= bounds.min_y() && drag_center.y() <= bounds.max_y();
                let center_in_x =
                    drag_center.x() >= bounds.min_x() && drag_center.x() <= bounds.max_x();
                let intersects_y = drag_position.max_y() >= bounds.min_y()
                    && drag_position.min_y() <= bounds.max_y();
                let intersects_x = drag_position.max_x() >= bounds.min_x()
                    && drag_position.min_x() <= bounds.max_x();

                let rank = if center_in_y && (center_in_x || intersects_x) {
                    0
                } else if intersects_y && intersects_x {
                    1
                } else {
                    return None;
                };
                let distance = (drag_center - bounds.center()).length();
                Some((rank, distance, project_key))
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
        let mut project_keys = Vec::new();
        for (_, _, project_key) in candidates {
            if !project_keys.contains(&project_key) {
                project_keys.push(project_key);
            }
        }

        if let Some(framework_target_project_key) = framework_target_project_key {
            if framework_target_project_key != source_project_key
                && !project_keys
                    .iter()
                    .any(|project_key| project_key == framework_target_project_key)
            {
                project_keys.push(framework_target_project_key.to_owned());
            }
        }

        project_keys
    }

    fn resolve_project_drop_target_for_drag(
        &self,
        source_project_key: &str,
        framework_target_project_key: Option<&str>,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) -> Option<AgentProjectDropTarget> {
        self.candidate_project_keys_for_drag(
            source_project_key,
            drag_position,
            framework_target_project_key,
            ctx,
        )
        .into_iter()
        .find_map(|target_project_key| {
            self.project_drop_target_for_drag(
                source_project_key,
                &target_project_key,
                drag_position,
                ctx,
            )
        })
    }

    fn move_project(
        &self,
        source_project_key: &str,
        target_project_key: &str,
        placement: AgentProjectMovePlacement,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let current_order = self
            .project_paths(ctx)
            .into_iter()
            .map(|path| project_order_key(&path))
            .collect::<Vec<_>>();
        let Some(next_order) = reorder_project_keys(
            &current_order,
            source_project_key,
            target_project_key,
            placement,
        ) else {
            return false;
        };

        *self.project_order.borrow_mut() = next_order;
        self.persist_project_order(ctx);
        true
    }

    fn persist_project_order(&self, ctx: &mut ViewContext<Self>) {
        let order = self.project_order.borrow();
        if let Ok(serialized) = serde_json::to_string(&*order) {
            if let Err(err) = ctx
                .private_user_preferences()
                .write_value(AGENT_SESSION_PROJECT_ORDER_PREF_KEY, serialized)
            {
                log::error!("Failed to persist agent project order: {err}");
            }
        }
    }

    fn persist_collapsed_project_keys(&self, ctx: &mut ViewContext<Self>) {
        let collapsed_project_keys = self.collapsed_project_keys.borrow();
        if let Ok(serialized) = serde_json::to_string(&*collapsed_project_keys) {
            if let Err(err) = ctx
                .private_user_preferences()
                .write_value(AGENT_SESSION_COLLAPSED_PROJECTS_PREF_KEY, serialized)
            {
                log::error!("Failed to persist collapsed agent projects: {err}");
            }
        }
    }

    fn mouse_state(&self, key: impl Into<String>) -> MouseStateHandle {
        let key = key.into();
        self.row_mouse_states
            .borrow_mut()
            .entry(key)
            .or_default()
            .clone()
    }

    fn draggable_state(&self, key: impl Into<String>) -> DraggableState {
        let key = key.into();
        self.row_draggable_states
            .borrow_mut()
            .entry(key)
            .or_default()
            .clone()
    }

    fn drop_target_for_drag(
        &self,
        source_session_id: &str,
        target_session_id: &str,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) -> Option<AgentSessionDropTarget> {
        if source_session_id == target_session_id {
            return None;
        }

        let target_position =
            ctx.element_position_by_id(agent_session_row_position_id(target_session_id))?;
        let target_height = target_position.height().max(1.);
        let top_boundary =
            target_position.min_y() + target_height * DROP_INTO_GROUP_VERTICAL_FRACTION;
        let bottom_boundary =
            target_position.max_y() - target_height * DROP_INTO_GROUP_VERTICAL_FRACTION;
        let drag_y = drag_position.center().y();
        let placement = if drag_y < top_boundary {
            AgentSessionMovePlacement::Before
        } else if drag_y > bottom_boundary {
            AgentSessionMovePlacement::After
        } else {
            AgentSessionMovePlacement::IntoGroup
        };

        let model = AgentSessionsModel::as_ref(ctx);
        let source = model.session(source_session_id)?;
        let target = model.session(target_session_id)?;
        let source_parent_id = model.parent_or_self_session_id(source_session_id)?;
        let target_parent_id = model.parent_or_self_session_id(target_session_id)?;
        let is_outdent_drag = source.parent_session_id.is_some()
            && ctx
                .element_position_by_id(agent_session_row_position_id(source_session_id))
                .is_some_and(|source_position| {
                    drag_position.min_x()
                        < source_position.min_x() - DROP_OUT_OF_GROUP_HORIZONTAL_OFFSET
                });

        if is_outdent_drag {
            if let Some(target_parent_session_id) = target.parent_session_id.as_ref() {
                return Some(AgentSessionDropTarget {
                    target_session_id: target_parent_session_id.clone(),
                    placement: AgentSessionMovePlacement::After,
                });
            }

            if placement == AgentSessionMovePlacement::IntoGroup
                && source_parent_id == target_parent_id
            {
                return Some(AgentSessionDropTarget {
                    target_session_id: target_session_id.to_owned(),
                    placement: AgentSessionMovePlacement::After,
                });
            }
        }

        if placement == AgentSessionMovePlacement::IntoGroup && source_parent_id == target_parent_id
        {
            return None;
        }

        let source_is_group_root = model
            .session(source_session_id)
            .is_some_and(|record| record.parent_session_id.is_none())
            && model.has_active_children(source_session_id);
        if source_is_group_root && target_parent_id == source_session_id {
            return None;
        }

        Some(AgentSessionDropTarget {
            target_session_id: target_session_id.to_owned(),
            placement,
        })
    }

    fn candidate_session_ids_for_drag(
        &self,
        source_session_id: &str,
        drag_position: RectF,
        framework_target_session_id: Option<&str>,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<String> {
        let drag_center = drag_position.center();
        let mut candidates = AgentSessionsModel::as_ref(ctx)
            .records()
            .iter()
            .filter(|record| record.id != source_session_id && !record.is_archived())
            .filter_map(|record| {
                let bounds =
                    ctx.element_position_by_id(agent_session_row_position_id(&record.id))?;
                let center_in_y =
                    drag_center.y() >= bounds.min_y() && drag_center.y() <= bounds.max_y();
                let center_in_x =
                    drag_center.x() >= bounds.min_x() && drag_center.x() <= bounds.max_x();
                let intersects_y = drag_position.max_y() >= bounds.min_y()
                    && drag_position.min_y() <= bounds.max_y();
                let intersects_x = drag_position.max_x() >= bounds.min_x()
                    && drag_position.min_x() <= bounds.max_x();

                let rank = if center_in_y && (center_in_x || intersects_x) {
                    0
                } else if intersects_y && intersects_x {
                    1
                } else {
                    return None;
                };
                let distance = (drag_center - bounds.center()).length();
                Some((rank, distance, record.id.clone()))
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
        let mut session_ids = Vec::new();
        for (_, _, session_id) in candidates {
            if !session_ids.contains(&session_id) {
                session_ids.push(session_id);
            }
        }

        if let Some(framework_target_session_id) = framework_target_session_id {
            if framework_target_session_id != source_session_id
                && !session_ids
                    .iter()
                    .any(|session_id| session_id == framework_target_session_id)
            {
                session_ids.push(framework_target_session_id.to_owned());
            }
        }

        session_ids
    }

    fn resolve_drop_target_for_drag(
        &self,
        source_session_id: &str,
        framework_target_session_id: Option<&str>,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) -> Option<AgentSessionDropTarget> {
        self.candidate_session_ids_for_drag(
            source_session_id,
            drag_position,
            framework_target_session_id,
            ctx,
        )
        .into_iter()
        .find_map(|target_session_id| {
            self.drop_target_for_drag(source_session_id, &target_session_id, drag_position, ctx)
        })
    }

    fn can_live_reorder(
        &self,
        source_session_id: &str,
        target_session_id: &str,
        placement: AgentSessionMovePlacement,
        ctx: &ViewContext<Self>,
    ) -> bool {
        if placement == AgentSessionMovePlacement::IntoGroup {
            return false;
        }

        let model = AgentSessionsModel::as_ref(ctx);
        let Some(source) = model.session(source_session_id) else {
            return false;
        };
        let Some(target) = model.session(target_session_id) else {
            return false;
        };

        source.parent_session_id == target.parent_session_id
            && source.project_path == target.project_path
    }

    fn handle_singleton_group_candidate(
        &mut self,
        parent_session_id: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(parent_session_id) = parent_session_id else {
            return;
        };
        if AgentSessionsModel::as_ref(ctx).active_group_session_count(&parent_session_id) > 1 {
            return;
        }

        match *TabSettings::as_ref(ctx)
            .singleton_agent_group_behavior
            .value()
        {
            SingletonAgentGroupBehavior::Ask => {
                self.pending_singleton_group_prompt = Some(SingletonGroupPrompt {
                    parent_session_id,
                    remember_choice: false,
                });
                ctx.notify();
            }
            SingletonAgentGroupBehavior::Disband => {
                ctx.dispatch_typed_action(&WorkspaceAction::DisbandAgentSessionGroup {
                    parent_session_id,
                });
            }
            SingletonAgentGroupBehavior::Keep => {}
        }
    }

    fn resolve_singleton_group_prompt(&mut self, disband: bool, ctx: &mut ViewContext<Self>) {
        let Some(prompt) = self.pending_singleton_group_prompt.take() else {
            return;
        };

        if prompt.remember_choice {
            let behavior = if disband {
                SingletonAgentGroupBehavior::Disband
            } else {
                SingletonAgentGroupBehavior::Keep
            };
            TabSettings::handle(ctx).update(ctx, |settings, ctx| {
                if let Err(err) = settings
                    .singleton_agent_group_behavior
                    .set_value(behavior, ctx)
                {
                    log::error!("Failed to update singleton agent group behavior: {err}");
                }
            });
        }

        if disband {
            ctx.dispatch_typed_action(&WorkspaceAction::DisbandAgentSessionGroup {
                parent_session_id: prompt.parent_session_id,
            });
        }
        ctx.notify();
    }

    fn request_delete_session(&mut self, session_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(record) = AgentSessionsModel::as_ref(ctx).session(session_id) else {
            return;
        };
        let title = record.title.clone();
        if self.renaming_session_id.as_deref() == Some(session_id) {
            self.cancel_rename(ctx);
        }
        self.pending_delete_confirmation = Some(PendingDeleteConfirmation::Session {
            session_id: session_id.to_owned(),
            title,
        });
    }

    fn request_delete_project(&mut self, project_path: &Path) {
        let name = project_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| project_path.to_string_lossy().to_string());
        self.pending_delete_confirmation = Some(PendingDeleteConfirmation::Project {
            project_path: project_path.to_path_buf(),
            name,
        });
    }

    fn confirm_pending_delete(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(pending_delete) = self.pending_delete_confirmation.take() else {
            return;
        };

        match pending_delete {
            PendingDeleteConfirmation::Session { session_id, .. } => {
                let singleton_group_id = AgentSessionsModel::as_ref(ctx)
                    .session(&session_id)
                    .and_then(|record| record.parent_session_id.clone())
                    .filter(|parent_session_id| {
                        AgentSessionsModel::as_ref(ctx)
                            .active_group_session_count(parent_session_id)
                            <= 2
                    });
                self.expanded_group_ids.borrow_mut().remove(&session_id);
                ctx.dispatch_typed_action(&WorkspaceAction::DeleteAgentSession {
                    session_id: session_id.clone(),
                });
                self.handle_singleton_group_candidate(singleton_group_id, ctx);
            }
            PendingDeleteConfirmation::Project { project_path, .. } => {
                self.collapsed_project_keys
                    .borrow_mut()
                    .remove(&project_order_key(&project_path));
                ctx.dispatch_typed_action(&WorkspaceAction::DeleteAgentSessionProject {
                    project_path,
                });
            }
        }
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
        active_terminal_view_id: Option<EntityId>,
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
        let project_path_buf = project_path.to_path_buf();
        let project_key = project_order_key(project_path);
        let is_collapsed = self.collapsed_project_keys.borrow().contains(&project_key);
        let project_draggable_state = self.draggable_state(format!("project:drag:{project_key}"));
        let is_dragging = self.active_drag_project_key.as_deref() == Some(project_key.as_str())
            || project_draggable_state.is_dragging();
        let is_actions_menu_open = self.open_project_actions_key.as_deref() == Some(&project_key);
        let drop_placement = self
            .hovered_project_drop_target
            .as_ref()
            .filter(|target| target.target_project_key == project_key)
            .map(|target| target.placement);
        let project_mouse_state =
            self.mouse_state(format!("project:{}", project_path.to_string_lossy()));
        let edit_project_mouse_state = self.mouse_state(format!(
            "project_action:{}:edit",
            project_path.to_string_lossy()
        ));
        let delete_project_mouse_state = self.mouse_state(format!(
            "project_action:{}:delete",
            project_path.to_string_lossy()
        ));
        let project_actions_button_mouse_state = self.mouse_state(format!(
            "project_action:{}:actions",
            project_path.to_string_lossy()
        ));

        let header_font_family = font_family.clone();
        let toggle_project_key = project_key.clone();
        let actions_project_key = project_key.clone();
        let menu_project_path = project_path_buf.clone();
        let header = Hoverable::new(project_mouse_state, move |state| {
            let actions = if state.is_hovered() || is_actions_menu_open {
                render_action_menu_button(
                    project_actions_button_mouse_state.clone(),
                    is_actions_menu_open,
                    AgentSessionsViewAction::ToggleProjectActionsMenu {
                        project_key: actions_project_key.clone(),
                    },
                    appearance,
                )
            } else {
                icon_button_placeholder()
            };

            let left_content = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(
                    ConstrainedBox::new(
                        if is_collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        }
                        .to_warpui_icon(theme.sub_text_color(theme.background()))
                        .finish(),
                    )
                    .with_width(11.)
                    .with_height(11.)
                    .finish(),
                )
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
                                Text::new_inline(
                                    project_name.clone(),
                                    header_font_family.clone(),
                                    12.,
                                )
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                            )
                            .with_child(
                                Text::new_inline(
                                    project_path_text.clone(),
                                    header_font_family.clone(),
                                    10.5,
                                )
                                .with_color(theme.sub_text_color(theme.background()).into())
                                .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
                )
                .finish();

            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.)
                    .with_child(Shrinkable::new(1.0, left_content).finish())
                    .with_child(actions)
                    .finish(),
            )
            .with_horizontal_padding(6.)
            .with_vertical_padding(5.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }
            if drop_placement.is_some() {
                container = container
                    .with_background(theme.surface_overlay_2())
                    .with_border(Border::all(1.).with_border_fill(theme.active_ui_detail()));
            } else if is_dragging {
                container = container.with_background(theme.surface_overlay_1());
            }

            let row = container.finish();
            if is_actions_menu_open {
                let mut stack = Stack::new().with_child(row);
                stack.add_positioned_overlay_child(
                    render_dismissible_action_menu(render_project_actions_menu(
                        &menu_project_path,
                        edit_project_mouse_state.clone(),
                        delete_project_mouse_state.clone(),
                        appearance,
                    )),
                    OffsetPositioning::offset_from_parent(
                        vec2f(-4., 28.),
                        ParentOffsetBounds::WindowByPosition,
                        ParentAnchor::TopRight,
                        ChildAnchor::TopRight,
                    ),
                );
                stack.finish()
            } else {
                row
            }
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(AgentSessionsViewAction::ToggleProject {
                project_key: toggle_project_key.clone(),
            });
        })
        .with_defer_events_to_children()
        .finish();

        let mut project_column = Flex::column().with_spacing(7.).with_child(header);

        if !is_collapsed {
            let reasoning_effort = AgentReasoningEffortModel::as_ref(app).effort();
            let agent_row = Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_children(SUPPORTED_AGENTS.into_iter().map(|agent| {
                    self.render_agent_chip(project_path, agent, reasoning_effort, app)
                }))
                .finish();
            project_column.add_child(agent_row);

            let active_sessions = sessions
                .iter()
                .filter(|session| !session.is_archived())
                .cloned()
                .collect::<Vec<_>>();
            let archived_sessions = sessions
                .iter()
                .filter(|session| session.is_archived())
                .cloned()
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
                let mut children_by_parent: HashMap<String, Vec<AgentSessionRecord>> =
                    HashMap::new();
                let active_ids = active_sessions
                    .iter()
                    .map(|session| session.id.clone())
                    .collect::<BTreeSet<_>>();
                let mut root_sessions = Vec::new();

                for session in active_sessions {
                    if let Some(parent_session_id) = session.parent_session_id.as_deref() {
                        if active_ids.contains(parent_session_id) {
                            children_by_parent
                                .entry(parent_session_id.to_owned())
                                .or_default()
                                .push(session.clone());
                            continue;
                        }
                    }
                    root_sessions.push(session.clone());
                }

                sort_sessions(&mut root_sessions);

                for children in children_by_parent.values_mut() {
                    sort_sessions(children);
                }

                for session in root_sessions {
                    let children = children_by_parent.remove(&session.id).unwrap_or_default();
                    let is_expanded = self.expanded_group_ids.borrow().contains(&session.id);
                    project_column.add_child(self.render_session_row(
                        &session,
                        &children,
                        false,
                        active_terminal_view_id,
                        app,
                    ));
                    if is_expanded {
                        if !children.is_empty() || session.group_terminal_view_id.is_some() {
                            project_column.add_child(self.render_group_member_session_row(
                                &session,
                                active_terminal_view_id,
                                app,
                            ));
                        }
                        for child in children {
                            project_column.add_child(self.render_session_row(
                                &child,
                                &[],
                                true,
                                active_terminal_view_id,
                                app,
                            ));
                        }
                    }
                }
                if !archived_sessions.is_empty() {
                    project_column.add_child(render_section_label("Archived", app));
                    for session in archived_sessions {
                        project_column.add_child(self.render_session_row(
                            &session,
                            &[],
                            false,
                            active_terminal_view_id,
                            app,
                        ));
                    }
                }
            }
        }

        let project = Container::new(project_column.finish())
            .with_horizontal_padding(SIDEBAR_HORIZONTAL_PADDING)
            .with_vertical_padding(6.)
            .finish();

        let row_position_id = agent_project_position_id(&project_key);
        let drag_project_key = project_key.clone();
        let drag_project_key_for_move = project_key.clone();
        let drag_project_key_for_drop = project_key.clone();
        let project = Draggable::new(project_draggable_state, project)
            .with_accepted_by_drop_target_fn(|drop_target_data, _| {
                if drop_target_data.as_any().is::<AgentProjectDropTargetData>() {
                    AcceptedByDropTarget::Yes
                } else {
                    AcceptedByDropTarget::No
                }
            })
            .on_drag_start(move |ctx, _, _| {
                ctx.dispatch_typed_action(AgentSessionsViewAction::StartProjectDrag {
                    project_key: drag_project_key.clone(),
                });
            })
            .on_drag(move |ctx, _, position, data| {
                let target_project_key = data
                    .and_then(|data| data.as_any().downcast_ref::<AgentProjectDropTargetData>())
                    .map(|data| data.project_key.clone());
                ctx.dispatch_typed_action(AgentSessionsViewAction::DragProject {
                    project_key: drag_project_key_for_move.clone(),
                    position,
                    target_project_key,
                });
            })
            .on_drop(move |ctx, _, position, data| {
                let target_project_key = data
                    .and_then(|data| data.as_any().downcast_ref::<AgentProjectDropTargetData>())
                    .map(|data| data.project_key.clone());
                ctx.dispatch_typed_action(AgentSessionsViewAction::DropProject {
                    project_key: drag_project_key_for_drop.clone(),
                    position,
                    target_project_key,
                });
            })
            .with_drag_axis(DragAxis::VerticalOnly)
            .with_defer_to_handled_child_mouse_down()
            .finish();

        let project = if is_dragging {
            Container::new(project)
                .with_background(theme.surface_overlay_1())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish()
        } else {
            project
        };

        let project = SavePosition::new(project, &row_position_id).finish();
        if is_dragging {
            project
        } else {
            DropTarget::new(project, AgentProjectDropTargetData { project_key }).finish()
        }
    }

    fn render_agent_chip(
        &self,
        project_path: &Path,
        agent: CLIAgent,
        reasoning_effort: AgentReasoningEffort,
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
                reasoning_effort,
            });
        })
        .finish()
    }

    fn render_session_row(
        &self,
        session: &AgentSessionRecord,
        children: &[AgentSessionRecord],
        is_child: bool,
        active_terminal_view_id: Option<EntityId>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        self.render_session_row_internal(
            session,
            children,
            is_child,
            false,
            active_terminal_view_id,
            app,
        )
    }

    fn render_group_member_session_row(
        &self,
        session: &AgentSessionRecord,
        active_terminal_view_id: Option<EntityId>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        self.render_session_row_internal(session, &[], true, true, active_terminal_view_id, app)
    }

    fn render_session_row_internal(
        &self,
        session: &AgentSessionRecord,
        children: &[AgentSessionRecord],
        is_child: bool,
        is_group_self_member: bool,
        active_terminal_view_id: Option<EntityId>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let font_family = appearance.ui_font_family();
        let row_state_key = if is_group_self_member {
            format!("session_member:{}", session.id)
        } else {
            format!("session:{}", session.id)
        };
        let mouse_state = self.mouse_state(row_state_key.clone());
        let draggable_state = self.draggable_state(row_state_key.clone());
        let session_id = session.id.clone();
        let restore_session_id = session.id.clone();
        let title = session.title.clone();
        let is_pinned = session.is_pinned;
        let is_archived = session.is_archived();
        let is_group =
            (!children.is_empty() || session.group_terminal_view_id.is_some()) && !is_child;
        let is_selected =
            Self::session_matches_terminal(session, children, active_terminal_view_id);
        let is_expanded = self.expanded_group_ids.borrow().contains(&session.id);
        let session_count = children.len() + 1;
        let is_renaming = self.renaming_session_id.as_deref() == Some(session.id.as_str());
        let is_dragging = !is_group_self_member
            && (self.active_drag_session_id.as_deref() == Some(session.id.as_str())
                || draggable_state.is_dragging());
        let is_actions_menu_open = self.open_session_actions_id.as_deref() == Some(&session.id);
        let drop_placement = self
            .hovered_drop_target
            .as_ref()
            .filter(|target| target.target_session_id == session.id)
            .map(|target| target.placement);
        let meta = if is_group {
            format!(
                "{} - {} {}",
                session.agent.display_name(),
                session_count,
                if session_count == 1 {
                    "session"
                } else {
                    "sessions"
                }
            )
        } else if is_archived {
            format!("{} - Archived", session.agent.display_name())
        } else {
            format!(
                "{} - {}",
                session.agent.display_name(),
                session.status.label()
            )
        };
        let status_fill = status_fill(session.status, app);
        let icon = if is_group {
            Icon::Grid
        } else {
            session.agent.icon().unwrap_or(Icon::Terminal)
        };
        let icon_fill = if is_group {
            theme.sub_text_color(theme.background())
        } else {
            status_fill
        };
        let group_toggle_state = self.mouse_state(format!("session_group_toggle:{row_state_key}"));
        let new_child_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:new_child"));
        let pin_button_state = self.mouse_state(format!("session_action:{row_state_key}:pin"));
        let rename_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:rename"));
        let archive_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:archive"));
        let disband_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:disband"));
        let delete_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:delete"));
        let actions_button_state =
            self.mouse_state(format!("session_action:{row_state_key}:actions"));
        let rename_editor = self.rename_editor.clone();

        // Snapshot the session's lifecycle for the click handler. Rendering is
        // the safest place to inspect the TerminalView's block state (we have
        // a `&AppContext` and can hold a short lock without contending with
        // event-driven paths), so we classify the session here:
        //
        //   * NotStarted — no `terminal_view_id` ever bound in this process.
        //     Happens for sessions that were never opened, and for records
        //     reloaded after a Warp restart (`#[serde(skip)]` drops the id).
        //     The user expects the row to surface the session: launch.
        //
        //   * Running — the terminal is still running its block. Includes
        //     the brief 0-1s window where `is_long_running` lags because
        //     `LONG_RUNNING_COMMAND_DURATION_MS` hasn't elapsed yet, which
        //     is why we read the block state directly instead of trusting
        //     the cached `was_long_running` flag.
        //
        //   * Dead — the terminal's block is no longer executing
        //     (DoneWithExecution / DoneWithNoExecution / Static / no active
        //     block). Typical case: the user `Ctrl+D`'d out of codex, or it
        //     crashed. Relaunching the agent matches user intent.
        let lifecycle_state = compute_session_lifecycle_state(session, app);

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

            let group_toggle = if is_group {
                render_group_toggle_button(
                    group_toggle_state.clone(),
                    is_expanded,
                    session_id.clone(),
                    appearance,
                )
            } else {
                ConstrainedBox::new(Empty::new().finish())
                    .with_width(12.)
                    .with_height(SESSION_ACTION_BUTTON_SIZE)
                    .finish()
            };

            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.)
                    .with_child(group_toggle)
                    .with_child(
                        ConstrainedBox::new(icon.to_warpui_icon(icon_fill).finish())
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
                    .finish(),
            )
            .with_horizontal_padding(8.)
            .with_vertical_padding(7.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if is_selected {
                container = container
                    .with_background(internal_colors::fg_overlay_2(theme))
                    .with_border(
                        Border::all(1.).with_border_fill(internal_colors::fg_overlay_3(theme)),
                    );
            }
            if state.is_hovered() {
                container = container.with_background(if is_selected {
                    internal_colors::fg_overlay_2(theme)
                } else {
                    internal_colors::fg_overlay_1(theme)
                });
            }
            if drop_placement.is_some() {
                container = container
                    .with_background(theme.surface_overlay_2())
                    .with_border(Border::all(1.).with_border_fill(theme.active_ui_detail()));
            } else if is_dragging {
                container = container.with_background(theme.surface_overlay_1());
            }

            let row = container.finish();
            if (state.is_hovered() || is_actions_menu_open) && !is_renaming && !is_group_self_member
            {
                let mut stack = Stack::new().with_child(row);
                stack.add_positioned_child(
                    render_action_menu_button(
                        actions_button_state.clone(),
                        is_actions_menu_open,
                        AgentSessionsViewAction::ToggleSessionActionsMenu {
                            session_id: session_id.clone(),
                        },
                        appearance,
                    ),
                    OffsetPositioning::offset_from_parent(
                        vec2f(-8., 0.),
                        ParentOffsetBounds::ParentByPosition,
                        ParentAnchor::MiddleRight,
                        ChildAnchor::MiddleRight,
                    ),
                );
                if is_actions_menu_open {
                    stack.add_positioned_overlay_child(
                        render_dismissible_action_menu(render_session_actions_menu(
                            &session_id,
                            !is_archived,
                            is_group,
                            is_pinned,
                            is_archived,
                            new_child_button_state.clone(),
                            pin_button_state.clone(),
                            rename_button_state.clone(),
                            archive_button_state.clone(),
                            disband_button_state.clone(),
                            delete_button_state.clone(),
                            appearance,
                        )),
                        OffsetPositioning::offset_from_parent(
                            vec2f(-4., 28.),
                            ParentOffsetBounds::WindowByPosition,
                            ParentAnchor::TopRight,
                            ChildAnchor::TopRight,
                        ),
                    );
                }
                stack.finish()
            } else {
                row
            }
        });

        let hoverable = if is_renaming {
            hoverable
        } else {
            hoverable
                .with_cursor(Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    if is_group && !is_group_self_member {
                        ctx.dispatch_typed_action(WorkspaceAction::RestoreAgentSessionGroup {
                            parent_session_id: restore_session_id.clone(),
                        });
                    } else if matches!(lifecycle_state, SessionLifecycle::Running) {
                        // Block is still executing (or in a Background state)
                        // — the agent is alive. Just bring the terminal to
                        // the front; never relaunch, the user may be in the
                        // middle of a conversation.
                        ctx.dispatch_typed_action(WorkspaceAction::FocusAgentSession {
                            session_id: restore_session_id.clone(),
                        });
                    } else {
                        // NotStarted (never attached in this process) or
                        // Dead (Ctrl+D'd / crashed / never came up). In both
                        // cases the user-visible expectation is "make this
                        // session usable", so reopen via the restore path.
                        ctx.dispatch_typed_action(WorkspaceAction::RestoreAgentSession {
                            session_id: restore_session_id.clone(),
                        });
                    }
                })
        };

        let mut row = hoverable.with_defer_events_to_children().finish();
        if is_child {
            row = Container::new(row).with_margin_left(18.).finish();
        }

        if is_group_self_member {
            return row;
        }

        let row_position_id = agent_session_row_position_id(&session.id);
        let row = if is_renaming || is_archived {
            row
        } else {
            let drag_session_id = session.id.clone();
            let drag_session_id_for_move = session.id.clone();
            let drag_session_id_for_drop = session.id.clone();
            Draggable::new(draggable_state, row)
                .with_accepted_by_drop_target_fn(|drop_target_data, _| {
                    if drop_target_data.as_any().is::<AgentSessionDropTargetData>() {
                        AcceptedByDropTarget::Yes
                    } else {
                        AcceptedByDropTarget::No
                    }
                })
                .on_drag_start(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AgentSessionsViewAction::StartDrag {
                        session_id: drag_session_id.clone(),
                    });
                })
                .on_drag(move |ctx, _, position, data| {
                    let target_session_id = data
                        .and_then(|data| data.as_any().downcast_ref::<AgentSessionDropTargetData>())
                        .map(|data| data.session_id.clone());
                    ctx.dispatch_typed_action(AgentSessionsViewAction::Drag {
                        session_id: drag_session_id_for_move.clone(),
                        position,
                        target_session_id,
                    });
                })
                .on_drop(move |ctx, _, position, data| {
                    let target_session_id = data
                        .and_then(|data| data.as_any().downcast_ref::<AgentSessionDropTargetData>())
                        .map(|data| data.session_id.clone());
                    ctx.dispatch_typed_action(AgentSessionsViewAction::Drop {
                        session_id: drag_session_id_for_drop.clone(),
                        position,
                        target_session_id,
                    });
                })
                .with_defer_to_handled_child_mouse_down()
                .finish()
        };

        let row = SavePosition::new(row, &row_position_id).finish();
        if is_dragging {
            row
        } else {
            DropTarget::new(
                row,
                AgentSessionDropTargetData {
                    session_id: session.id.clone(),
                },
            )
            .finish()
        }
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

    fn render_ssh_remote_loading_state(
        &self,
        host: &SshRemoteHost,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let phase = self.ssh_remote_loading_tick as usize;
        let active_index = phase % 4;

        let status_row = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    ConstrainedBox::new(Icon::Loading.to_warpui_icon(theme.accent()).finish())
                        .with_width(15.)
                        .with_height(15.)
                        .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Flex::column()
                            .with_spacing(2.)
                            .with_child(
                                Text::new_inline(
                                    "Connecting remote",
                                    appearance.ui_font_family(),
                                    12.,
                                )
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                            )
                            .with_child(
                                Text::new_inline(
                                    host.display_name().to_owned(),
                                    appearance.ui_font_family(),
                                    10.5,
                                )
                                .with_color(theme.sub_text_color(theme.background()).into())
                                .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(8.)
        .with_vertical_padding(8.)
        .with_background(theme.surface_overlay_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .with_horizontal_margin(SIDEBAR_HORIZONTAL_PADDING)
        .finish();

        let mut content = Flex::column()
            .with_spacing(8.)
            .with_child(self.render_projects_header(app))
            .with_child(status_row);

        for index in 0..2 {
            content.add_child(render_ssh_remote_project_skeleton(
                &appearance,
                active_index,
                index,
            ));
        }

        content.finish()
    }

    fn render_singleton_group_prompt(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let prompt = self.pending_singleton_group_prompt.as_ref()?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let remember_checked = prompt.remember_choice;

        let checkbox = appearance
            .ui_builder()
            .checkbox(
                self.singleton_prompt_remember_mouse_state.clone(),
                Some(13.),
            )
            .with_label(Span::new("Remember this choice", Default::default()))
            .check(remember_checked)
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(
                    AgentSessionsViewAction::ToggleSingletonGroupPromptRemember,
                );
            })
            .finish();

        let content = Container::new(
            Flex::column()
                .with_spacing(10.)
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new_inline(
                                "Disband single-session group?",
                                appearance.ui_font_family(),
                                12.,
                            )
                            .with_color(theme.main_text_color(theme.background()).into())
                            .with_style(Properties::default().weight(Weight::Semibold))
                            .finish(),
                        )
                        .with_child(render_session_action_button(
                            self.singleton_prompt_close_mouse_state.clone(),
                            Icon::X,
                            "Close",
                            AgentSessionsViewAction::CancelSingletonGroupPrompt,
                            false,
                            false,
                            appearance,
                        ))
                        .finish(),
                )
                .with_child(
                    Text::new(
                        "This group now contains one session. You can keep the group shell or disband it.",
                        appearance.ui_font_family(),
                        11.,
                    )
                    .with_color(theme.sub_text_color(theme.background()).into_solid())
                    .finish(),
                )
                .with_child(checkbox)
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(6.)
                        .with_child(render_prompt_button(
                            self.singleton_prompt_keep_mouse_state.clone(),
                            "Keep",
                            false,
                            AgentSessionsViewAction::ResolveSingletonGroupPrompt { disband: false },
                            appearance,
                        ))
                        .with_child(render_prompt_button(
                            self.singleton_prompt_disband_mouse_state.clone(),
                            "Disband",
                            true,
                            AgentSessionsViewAction::ResolveSingletonGroupPrompt { disband: true },
                            appearance,
                        ))
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(12.)
        .with_vertical_padding(12.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        Some(
            ConstrainedBox::new(content)
                .with_width(SINGLETON_GROUP_PROMPT_WIDTH)
                .finish(),
        )
    }

    fn render_delete_confirmation_prompt(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let pending_delete = self.pending_delete_confirmation.as_ref()?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let (title, body) = match pending_delete {
            PendingDeleteConfirmation::Session { title, .. } => (
                "Delete session?",
                format!("\"{}\" will be removed from this project.", title),
            ),
            PendingDeleteConfirmation::Project { name, .. } => (
                "Delete project?",
                format!(
                    "\"{}\" and its agent sessions will be removed from the sidebar.",
                    name
                ),
            ),
        };

        let content = Container::new(
            Flex::column()
                .with_spacing(10.)
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new_inline(title, appearance.ui_font_family(), 12.)
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .with_child(render_session_action_button(
                            self.delete_prompt_close_mouse_state.clone(),
                            Icon::X,
                            "Close",
                            AgentSessionsViewAction::CancelDeleteConfirmation,
                            false,
                            false,
                            appearance,
                        ))
                        .finish(),
                )
                .with_child(
                    Text::new(body, appearance.ui_font_family(), 11.)
                        .with_color(theme.sub_text_color(theme.background()).into_solid())
                        .finish(),
                )
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(6.)
                        .with_child(render_prompt_button(
                            self.delete_prompt_cancel_mouse_state.clone(),
                            "Cancel",
                            false,
                            AgentSessionsViewAction::CancelDeleteConfirmation,
                            appearance,
                        ))
                        .with_child(render_danger_prompt_button(
                            self.delete_prompt_delete_mouse_state.clone(),
                            "Delete",
                            AgentSessionsViewAction::ConfirmPendingDelete,
                            appearance,
                        ))
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(12.)
        .with_vertical_padding(12.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        Some(
            ConstrainedBox::new(content)
                .with_width(DELETE_CONFIRMATION_PROMPT_WIDTH)
                .finish(),
        )
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
        if let Some(host) = Self::connecting_ssh_remote_host(app) {
            return self.render_ssh_remote_loading_state(&host, app);
        }

        let environment_id = Self::current_environment_id(app);
        let active_terminal_view_id = self.active_terminal_view_id(app);
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
                .filter(|record| {
                    record.environment_id == environment_id && record.project_path == project_path
                })
                .cloned()
                .collect::<Vec<_>>();
            sort_sessions(&mut sessions);
            content.add_child(self.render_project(
                &project_path,
                &sessions,
                active_terminal_view_id,
                app,
            ));
        }

        let scrollable = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            content.finish(),
            ScrollbarWidth::Auto,
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            ElementFill::None,
        )
        .with_overlayed_scrollbar()
        .finish();

        let prompt = self
            .render_delete_confirmation_prompt(app)
            .or_else(|| self.render_singleton_group_prompt(app));

        if let Some(prompt) = prompt {
            let mut stack = Stack::new().with_child(scrollable);
            stack.add_positioned_child(
                prompt,
                OffsetPositioning::offset_from_parent(
                    vec2f(SIDEBAR_HORIZONTAL_PADDING, SINGLETON_GROUP_PROMPT_OFFSET),
                    ParentOffsetBounds::ParentByPosition,
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
            stack.finish()
        } else {
            scrollable
        }
    }
}

impl TypedActionView for AgentSessionsView {
    type Action = AgentSessionsViewAction;

    fn handle_action(&mut self, action: &AgentSessionsViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            AgentSessionsViewAction::TogglePin { session_id } => {
                self.open_session_actions_id = None;
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.toggle_pin(session_id, ctx);
                });
            }
            AgentSessionsViewAction::BeginRename { session_id } => {
                self.open_session_actions_id = None;
                self.begin_rename(session_id, ctx);
            }
            AgentSessionsViewAction::ToggleArchive { session_id } => {
                self.open_session_actions_id = None;
                if self.renaming_session_id.as_deref() == Some(session_id.as_str()) {
                    self.cancel_rename(ctx);
                }
                let singleton_group_id = AgentSessionsModel::as_ref(ctx)
                    .session(session_id)
                    .and_then(|record| record.parent_session_id.clone())
                    .filter(|parent_session_id| {
                        AgentSessionsModel::as_ref(ctx)
                            .active_group_session_count(parent_session_id)
                            <= 2
                    });
                AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.toggle_archive(session_id, ctx);
                });
                self.handle_singleton_group_candidate(singleton_group_id, ctx);
            }
            AgentSessionsViewAction::Delete { session_id } => {
                self.open_session_actions_id = None;
                self.request_delete_session(session_id, ctx);
            }
            AgentSessionsViewAction::EditProject { project_path } => {
                self.open_project_actions_key = None;
                ctx.dispatch_typed_action(&WorkspaceAction::EditAgentSessionProject {
                    project_path: project_path.clone(),
                });
            }
            AgentSessionsViewAction::DeleteProject { project_path } => {
                self.open_project_actions_key = None;
                self.request_delete_project(project_path);
            }
            AgentSessionsViewAction::DisbandGroup { session_id } => {
                self.open_session_actions_id = None;
                let parent_session_id = AgentSessionsModel::as_ref(ctx)
                    .parent_or_self_session_id(session_id)
                    .unwrap_or_else(|| session_id.clone());
                self.expanded_group_ids
                    .borrow_mut()
                    .remove(&parent_session_id);
                ctx.dispatch_typed_action(&WorkspaceAction::DisbandAgentSessionGroup {
                    parent_session_id,
                });
            }
            AgentSessionsViewAction::ToggleGroup { session_id } => {
                let parent_session_id = AgentSessionsModel::as_ref(ctx)
                    .parent_or_self_session_id(session_id)
                    .unwrap_or_else(|| session_id.clone());
                let mut expanded_group_ids = self.expanded_group_ids.borrow_mut();
                if !expanded_group_ids.insert(parent_session_id.clone()) {
                    expanded_group_ids.remove(&parent_session_id);
                }
            }
            AgentSessionsViewAction::ToggleProject { project_key } => {
                {
                    let mut collapsed_project_keys = self.collapsed_project_keys.borrow_mut();
                    if !collapsed_project_keys.insert(project_key.clone()) {
                        collapsed_project_keys.remove(project_key);
                    }
                }
                self.persist_collapsed_project_keys(ctx);
            }
            AgentSessionsViewAction::ToggleSessionActionsMenu { session_id } => {
                self.open_project_actions_key = None;
                if self.open_session_actions_id.as_deref() == Some(session_id.as_str()) {
                    self.open_session_actions_id = None;
                } else {
                    self.open_session_actions_id = Some(session_id.clone());
                }
            }
            AgentSessionsViewAction::ToggleProjectActionsMenu { project_key } => {
                self.open_session_actions_id = None;
                if self.open_project_actions_key.as_deref() == Some(project_key.as_str()) {
                    self.open_project_actions_key = None;
                } else {
                    self.open_project_actions_key = Some(project_key.clone());
                }
            }
            AgentSessionsViewAction::CloseActionsMenu => {
                self.open_session_actions_id = None;
                self.open_project_actions_key = None;
            }
            AgentSessionsViewAction::StartChild { session_id } => {
                self.open_session_actions_id = None;
                let Some(parent_session_id) =
                    AgentSessionsModel::as_ref(ctx).parent_or_self_session_id(session_id)
                else {
                    return;
                };
                self.expanded_group_ids
                    .borrow_mut()
                    .insert(parent_session_id.clone());
                ctx.dispatch_typed_action(&WorkspaceAction::StartAgentChildSession {
                    parent_session_id,
                });
            }
            AgentSessionsViewAction::ResumeSession { session_id } => {
                self.open_session_actions_id = None;
                ctx.dispatch_typed_action(&WorkspaceAction::RestoreAgentSession {
                    session_id: session_id.clone(),
                });
            }
            AgentSessionsViewAction::StartDrag { session_id } => {
                self.open_session_actions_id = None;
                self.open_project_actions_key = None;
                self.active_drag_session_id = Some(session_id.clone());
                self.hovered_drop_target = None;
            }
            AgentSessionsViewAction::StartProjectDrag { project_key } => {
                self.open_session_actions_id = None;
                self.open_project_actions_key = None;
                self.active_drag_project_key = Some(project_key.clone());
                self.hovered_project_drop_target = None;
            }
            AgentSessionsViewAction::Drag {
                session_id,
                position,
                target_session_id,
            } => {
                let drop_target = self.resolve_drop_target_for_drag(
                    session_id,
                    target_session_id.as_deref(),
                    *position,
                    ctx,
                );
                if let Some(drop_target) = drop_target.as_ref() {
                    let should_live_reorder = self
                        .hovered_drop_target
                        .as_ref()
                        .is_none_or(|current| current != drop_target)
                        && self.can_live_reorder(
                            session_id,
                            &drop_target.target_session_id,
                            drop_target.placement,
                            ctx,
                        );

                    if should_live_reorder {
                        let outcome = AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                            model.move_session(
                                session_id,
                                &drop_target.target_session_id,
                                drop_target.placement,
                                ctx,
                            )
                        });
                        if let Some(outcome) = outcome {
                            if let Some(expanded_group_id) = outcome.expanded_group_id {
                                self.expanded_group_ids
                                    .borrow_mut()
                                    .insert(expanded_group_id);
                            }
                        }
                    }
                }
                self.hovered_drop_target = drop_target;
            }
            AgentSessionsViewAction::DragProject {
                project_key,
                position,
                target_project_key,
            } => {
                let drop_target = self.resolve_project_drop_target_for_drag(
                    project_key,
                    target_project_key.as_deref(),
                    *position,
                    ctx,
                );
                if let Some(drop_target) = drop_target.as_ref() {
                    let should_live_reorder = self
                        .hovered_project_drop_target
                        .as_ref()
                        .is_none_or(|current| current != drop_target);

                    if should_live_reorder {
                        self.move_project(
                            project_key,
                            &drop_target.target_project_key,
                            drop_target.placement,
                            ctx,
                        );
                    }
                }
                self.hovered_project_drop_target = drop_target;
            }
            AgentSessionsViewAction::Drop {
                session_id,
                position,
                target_session_id,
            } => {
                let drop_target = self
                    .resolve_drop_target_for_drag(
                        session_id,
                        target_session_id.as_deref(),
                        *position,
                        ctx,
                    )
                    .or_else(|| self.hovered_drop_target.clone());
                self.active_drag_session_id = None;
                self.hovered_drop_target = None;

                if let Some(drop_target) = drop_target {
                    let outcome = AgentSessionsModel::handle(ctx).update(ctx, |model, ctx| {
                        model.move_session(
                            session_id,
                            &drop_target.target_session_id,
                            drop_target.placement,
                            ctx,
                        )
                    });
                    if let Some(outcome) = outcome {
                        if let Some(expanded_group_id) = outcome.expanded_group_id {
                            self.expanded_group_ids
                                .borrow_mut()
                                .insert(expanded_group_id);
                        }
                        if !outcome.terminal_view_ids_to_detach.is_empty() {
                            ctx.dispatch_typed_action(
                                &WorkspaceAction::DetachAgentSessionTerminalViews {
                                    terminal_view_ids: outcome.terminal_view_ids_to_detach,
                                },
                            );
                        }
                        self.handle_singleton_group_candidate(outcome.singleton_group_id, ctx);
                    }
                }
            }
            AgentSessionsViewAction::DropProject {
                project_key,
                position,
                target_project_key,
            } => {
                let drop_target = self
                    .resolve_project_drop_target_for_drag(
                        project_key,
                        target_project_key.as_deref(),
                        *position,
                        ctx,
                    )
                    .or_else(|| self.hovered_project_drop_target.clone());
                self.active_drag_project_key = None;
                self.hovered_project_drop_target = None;

                if let Some(drop_target) = drop_target {
                    self.move_project(
                        project_key,
                        &drop_target.target_project_key,
                        drop_target.placement,
                        ctx,
                    );
                }
            }
            AgentSessionsViewAction::CancelSingletonGroupPrompt => {
                self.pending_singleton_group_prompt = None;
            }
            AgentSessionsViewAction::ToggleSingletonGroupPromptRemember => {
                if let Some(prompt) = self.pending_singleton_group_prompt.as_mut() {
                    prompt.remember_choice = !prompt.remember_choice;
                }
            }
            AgentSessionsViewAction::ResolveSingletonGroupPrompt { disband } => {
                self.resolve_singleton_group_prompt(*disband, ctx);
            }
            AgentSessionsViewAction::CancelDeleteConfirmation => {
                self.open_session_actions_id = None;
                self.open_project_actions_key = None;
                self.pending_delete_confirmation = None;
            }
            AgentSessionsViewAction::ConfirmPendingDelete => {
                self.open_session_actions_id = None;
                self.open_project_actions_key = None;
                self.confirm_pending_delete(ctx);
            }
        }
        ctx.notify();
    }
}

#[derive(Debug, Clone)]
pub enum AgentSessionsViewAction {
    TogglePin {
        session_id: String,
    },
    BeginRename {
        session_id: String,
    },
    ToggleArchive {
        session_id: String,
    },
    Delete {
        session_id: String,
    },
    EditProject {
        project_path: PathBuf,
    },
    DeleteProject {
        project_path: PathBuf,
    },
    DisbandGroup {
        session_id: String,
    },
    ToggleGroup {
        session_id: String,
    },
    ToggleProject {
        project_key: String,
    },
    ToggleSessionActionsMenu {
        session_id: String,
    },
    ToggleProjectActionsMenu {
        project_key: String,
    },
    CloseActionsMenu,
    StartChild {
        session_id: String,
    },
    ResumeSession {
        session_id: String,
    },
    StartDrag {
        session_id: String,
    },
    StartProjectDrag {
        project_key: String,
    },
    Drag {
        session_id: String,
        position: RectF,
        target_session_id: Option<String>,
    },
    DragProject {
        project_key: String,
        position: RectF,
        target_project_key: Option<String>,
    },
    Drop {
        session_id: String,
        position: RectF,
        target_session_id: Option<String>,
    },
    DropProject {
        project_key: String,
        position: RectF,
        target_project_key: Option<String>,
    },
    CancelSingletonGroupPrompt,
    ToggleSingletonGroupPromptRemember,
    ResolveSingletonGroupPrompt {
        disband: bool,
    },
    CancelDeleteConfirmation,
    ConfirmPendingDelete,
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
            .then_with(|| b.sort_order.cmp(&a.sort_order))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
}

fn agent_session_row_position_id(session_id: &str) -> String {
    format!("agent_session_row:{session_id}")
}

fn agent_project_position_id(project_key: &str) -> String {
    format!("agent_session_project:{project_key}")
}

fn project_order_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn reorder_project_keys(
    current_order: &[String],
    source_key: &str,
    target_key: &str,
    placement: AgentProjectMovePlacement,
) -> Option<Vec<String>> {
    if source_key == target_key
        || !current_order.iter().any(|key| key == source_key)
        || !current_order.iter().any(|key| key == target_key)
    {
        return None;
    }

    let mut next_order = current_order
        .iter()
        .filter(|key| key.as_str() != source_key)
        .cloned()
        .collect::<Vec<_>>();
    let target_index = next_order.iter().position(|key| key == target_key)?;
    let insert_index = match placement {
        AgentProjectMovePlacement::Before => target_index,
        AgentProjectMovePlacement::After => target_index.saturating_add(1).min(next_order.len()),
    };
    next_order.insert(insert_index, source_key.to_owned());

    if next_order == current_order {
        None
    } else {
        Some(next_order)
    }
}

fn read_project_order(ctx: &ViewContext<AgentSessionsView>) -> Vec<String> {
    ctx.private_user_preferences()
        .read_value(AGENT_SESSION_PROJECT_ORDER_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| serde_json::from_str::<Vec<String>>(&serialized).ok())
        .unwrap_or_default()
}

fn read_collapsed_project_keys(ctx: &ViewContext<AgentSessionsView>) -> BTreeSet<String> {
    ctx.private_user_preferences()
        .read_value(AGENT_SESSION_COLLAPSED_PROJECTS_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| serde_json::from_str::<BTreeSet<String>>(&serialized).ok())
        .unwrap_or_default()
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

fn render_dismissible_action_menu(menu: Box<dyn Element>) -> Box<dyn Element> {
    Dismiss::new(menu)
        .on_dismiss(|ctx, _app| {
            ctx.dispatch_typed_action(AgentSessionsViewAction::CloseActionsMenu);
        })
        .prevent_interaction_with_other_elements()
        .finish()
}

fn render_session_actions_menu(
    session_id: &str,
    can_start_child: bool,
    is_group: bool,
    is_pinned: bool,
    is_archived: bool,
    new_child_button_state: MouseStateHandle,
    pin_button_state: MouseStateHandle,
    rename_button_state: MouseStateHandle,
    archive_button_state: MouseStateHandle,
    disband_button_state: MouseStateHandle,
    delete_button_state: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mut menu = Flex::column().with_main_axis_size(MainAxisSize::Min);
    menu.add_child(render_action_menu_item(
        new_child_button_state.clone(),
        Icon::Refresh,
        "Resume session",
        AgentSessionsViewAction::ResumeSession {
            session_id: session_id.to_owned(),
        },
        false,
        appearance,
    ));
    if can_start_child {
        menu.add_child(render_action_menu_item(
            new_child_button_state,
            Icon::Plus,
            "New child agent",
            AgentSessionsViewAction::StartChild {
                session_id: session_id.to_owned(),
            },
            false,
            appearance,
        ));
    }
    menu.add_child(render_action_menu_item(
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
        false,
        appearance,
    ));
    menu.add_child(render_action_menu_item(
        rename_button_state,
        Icon::Rename,
        "Rename",
        AgentSessionsViewAction::BeginRename {
            session_id: session_id.to_owned(),
        },
        false,
        appearance,
    ));
    menu.add_child(render_action_menu_item(
        archive_button_state,
        Icon::Inbox,
        if is_archived { "Unarchive" } else { "Archive" },
        AgentSessionsViewAction::ToggleArchive {
            session_id: session_id.to_owned(),
        },
        false,
        appearance,
    ));
    if is_group {
        menu.add_child(render_action_menu_item(
            disband_button_state,
            Icon::Grid,
            "Disband group",
            AgentSessionsViewAction::DisbandGroup {
                session_id: session_id.to_owned(),
            },
            false,
            appearance,
        ));
    }
    menu.add_child(render_action_menu_item(
        delete_button_state,
        Icon::Trash,
        "Delete",
        AgentSessionsViewAction::Delete {
            session_id: session_id.to_owned(),
        },
        true,
        appearance,
    ));

    ConstrainedBox::new(
        Container::new(menu.finish())
            .with_vertical_padding(4.)
            .with_background(appearance.theme().surface_2())
            .with_border(Border::all(1.).with_border_fill(appearance.theme().surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish(),
    )
    .with_width(ACTION_MENU_WIDTH)
    .finish()
}

fn render_project_actions_menu(
    project_path: &Path,
    edit_project_mouse_state: MouseStateHandle,
    delete_project_mouse_state: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_child(render_action_menu_item(
                    edit_project_mouse_state,
                    Icon::Pencil,
                    "Edit project",
                    AgentSessionsViewAction::EditProject {
                        project_path: project_path.to_path_buf(),
                    },
                    false,
                    appearance,
                ))
                .with_child(render_action_menu_item(
                    delete_project_mouse_state,
                    Icon::Trash,
                    "Delete project",
                    AgentSessionsViewAction::DeleteProject {
                        project_path: project_path.to_path_buf(),
                    },
                    true,
                    appearance,
                ))
                .finish(),
        )
        .with_vertical_padding(4.)
        .with_background(appearance.theme().surface_2())
        .with_border(Border::all(1.).with_border_fill(appearance.theme().surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish(),
    )
    .with_width(ACTION_MENU_WIDTH)
    .finish()
}

fn render_group_toggle_button(
    mouse_state: MouseStateHandle,
    is_expanded: bool,
    session_id: String,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let icon_color = if state.is_hovered() {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        Container::new(
            Align::new(
                ConstrainedBox::new(
                    if is_expanded {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    }
                    .to_warpui_icon(icon_color)
                    .finish(),
                )
                .with_width(11.)
                .with_height(11.)
                .finish(),
            )
            .finish(),
        )
        .with_horizontal_padding(1.)
        .with_vertical_padding(3.)
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(AgentSessionsViewAction::ToggleGroup {
            session_id: session_id.clone(),
        });
    })
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

fn skeleton_fill(appearance: &Appearance, active: bool) -> ThemeFill {
    if active {
        appearance.theme().surface_overlay_2()
    } else {
        appearance.theme().surface_overlay_1()
    }
}

fn render_skeleton_bar(
    appearance: &Appearance,
    width: f32,
    height: f32,
    active: bool,
) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(
            Rect::new()
                .with_background(skeleton_fill(appearance, active))
                .finish(),
        )
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish(),
    )
    .with_width(width)
    .with_height(height)
    .finish()
}

fn render_skeleton_square(appearance: &Appearance, size: f32, active: bool) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(
            Rect::new()
                .with_background(skeleton_fill(appearance, active))
                .finish(),
        )
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish(),
    )
    .with_width(size)
    .with_height(size)
    .finish()
}

fn render_ssh_remote_session_skeleton(
    appearance: &Appearance,
    active_index: usize,
    project_index: usize,
    session_index: usize,
) -> Box<dyn Element> {
    let active = active_index == (project_index + session_index + 1) % 4;
    let title_width = if session_index == 0 { 132. } else { 108. };
    let subtitle_width = if session_index == 0 { 86. } else { 116. };

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(render_skeleton_square(appearance, 14., active))
            .with_child(
                Flex::column()
                    .with_spacing(4.)
                    .with_child(render_skeleton_bar(appearance, title_width, 9., active))
                    .with_child(render_skeleton_bar(appearance, subtitle_width, 7., !active))
                    .finish(),
            )
            .finish(),
    )
    .with_margin_left(24.)
    .with_vertical_padding(6.)
    .finish()
}

fn render_ssh_remote_project_skeleton(
    appearance: &Appearance,
    active_index: usize,
    project_index: usize,
) -> Box<dyn Element> {
    let active = active_index == project_index % 4;
    let theme = appearance.theme();
    let title_width = if project_index == 0 { 92. } else { 118. };
    let path_width = if project_index == 0 { 64. } else { 78. };

    let project_header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.)
        .with_child(
            ConstrainedBox::new(
                Icon::ChevronDown
                    .to_warpui_icon(theme.disabled_text_color(theme.background()))
                    .finish(),
            )
            .with_width(11.)
            .with_height(11.)
            .finish(),
        )
        .with_child(
            ConstrainedBox::new(
                Icon::Folder
                    .to_warpui_icon(theme.disabled_text_color(theme.background()))
                    .finish(),
            )
            .with_width(14.)
            .with_height(14.)
            .finish(),
        )
        .with_child(
            Flex::column()
                .with_spacing(4.)
                .with_child(render_skeleton_bar(appearance, title_width, 9., active))
                .with_child(render_skeleton_bar(appearance, path_width, 7., !active))
                .finish(),
        )
        .finish();

    Container::new(
        Flex::column()
            .with_spacing(1.)
            .with_child(
                Container::new(project_header)
                    .with_vertical_padding(5.)
                    .finish(),
            )
            .with_child(render_ssh_remote_session_skeleton(
                appearance,
                active_index,
                project_index,
                0,
            ))
            .with_child(render_ssh_remote_session_skeleton(
                appearance,
                active_index,
                project_index,
                1,
            ))
            .finish(),
    )
    .with_horizontal_margin(SIDEBAR_HORIZONTAL_PADDING)
    .with_vertical_padding(2.)
    .finish()
}

fn render_prompt_button(
    mouse_state: MouseStateHandle,
    label: &'static str,
    is_primary: bool,
    action: AgentSessionsViewAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let background = if is_primary {
            ThemeFill::Solid(theme.accent().into())
        } else if state.is_hovered() {
            theme.surface_overlay_2()
        } else {
            theme.surface_overlay_1()
        };
        let text_fill = if is_primary {
            theme.background().into_solid()
        } else {
            theme.main_text_color(theme.background()).into_solid()
        };

        Container::new(
            Text::new_inline(label, appearance.ui_font_family(), 11.)
                .with_color(text_fill)
                .with_style(Properties::default().weight(Weight::Medium))
                .finish(),
        )
        .with_horizontal_padding(9.)
        .with_vertical_padding(5.)
        .with_background(background)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_danger_prompt_button(
    mouse_state: MouseStateHandle,
    label: &'static str,
    action: AgentSessionsViewAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let background = if state.is_hovered() {
            ThemeFill::Solid(theme.ansi_fg_red())
        } else {
            theme.surface_overlay_1()
        };
        let text_fill: ColorU = if state.is_hovered() {
            theme.background().into()
        } else {
            theme.ansi_fg_red()
        };

        Container::new(
            Text::new_inline(label, appearance.ui_font_family(), 11.)
                .with_color(text_fill)
                .with_style(Properties::default().weight(Weight::Medium))
                .finish(),
        )
        .with_horizontal_padding(9.)
        .with_vertical_padding(5.)
        .with_background(background)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .finish()
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

fn render_action_menu_button(
    mouse_state: MouseStateHandle,
    is_open: bool,
    action: AgentSessionsViewAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let icon_color = if state.is_hovered() || is_open {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let mut button = Container::new(
            Align::new(
                ConstrainedBox::new(Icon::DotsVertical.to_warpui_icon(icon_color).finish())
                    .with_width(13.)
                    .with_height(13.)
                    .finish(),
            )
            .finish(),
        )
        .with_horizontal_padding(4.)
        .with_vertical_padding(4.)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if state.is_hovered() || is_open {
            button = button.with_background(theme.surface_overlay_1());
        }

        ConstrainedBox::new(button.finish())
            .with_width(ICON_BUTTON_SIZE)
            .with_height(ICON_BUTTON_SIZE)
            .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_action_menu_item(
    mouse_state: MouseStateHandle,
    icon: Icon,
    label: &'static str,
    action: AgentSessionsViewAction,
    is_danger: bool,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    Hoverable::new(mouse_state, move |state| {
        let item_color = if is_danger && state.is_hovered() {
            ThemeFill::Solid(theme.ansi_fg_red())
        } else if state.is_hovered() {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let mut row = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    ConstrainedBox::new(icon.to_warpui_icon(item_color).finish())
                        .with_width(13.)
                        .with_height(13.)
                        .finish(),
                )
                .with_child(
                    Text::new_inline(label, appearance.ui_font_family(), 11.)
                        .with_color(item_color.into_solid())
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(9.)
        .with_vertical_padding(7.)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

        if state.is_hovered() {
            row = row.with_background(theme.surface_overlay_1());
        }

        row.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
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
    let mut records = ctx
        .private_user_preferences()
        .read_value(AGENT_SESSION_RECORDS_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| serde_json::from_str::<Vec<AgentSessionRecord>>(&serialized).ok())
        .unwrap_or_default()
        .into_iter()
        .take(MAX_AGENT_SESSION_RECORDS)
        .collect::<Vec<_>>();

    for record in &mut records {
        if record.sort_order == 0 {
            record.sort_order = record.updated_at_ms;
        }
    }

    records
}

fn read_project_records(ctx: &ModelContext<AgentSessionsModel>) -> Vec<AgentSessionProjectRecord> {
    ctx.private_user_preferences()
        .read_value(AGENT_SESSION_PROJECTS_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| {
            serde_json::from_str::<Vec<AgentSessionProjectRecord>>(&serialized).ok()
        })
        .unwrap_or_default()
}

fn friendly_path(path: &Path) -> String {
    let raw_path = path.to_string_lossy();
    let home = dirs::home_dir().and_then(|path| path.to_str().map(str::to_owned));
    user_friendly_path(&raw_path, home.as_deref()).into_owned()
}

fn new_session_title(agent: CLIAgent) -> String {
    format!("New {} session", agent.display_name())
}

#[derive(Debug, Clone)]
struct AgentSessionTitleRequest {
    agent: CLIAgent,
    project_path: PathBuf,
    fingerprint: u64,
    prompt: String,
    first_prompt_title: Option<String>,
    fallback_title: String,
    source_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoTitleAction {
    FirstPrompt,
    Refresh,
}

fn auto_title_action(
    record: &AgentSessionRecord,
    request: &AgentSessionTitleRequest,
    now_ms: i64,
) -> Option<AutoTitleAction> {
    if record.title_overridden || record.auto_title_fingerprint == Some(request.fingerprint) {
        return None;
    }

    if record.auto_title_fingerprint.is_none() && request.first_prompt_title.is_some() {
        return Some(AutoTitleAction::FirstPrompt);
    }

    let Some(last_refresh_ms) = record.auto_title_summarized_at_ms else {
        return Some(AutoTitleAction::Refresh);
    };
    if now_ms.saturating_sub(last_refresh_ms) >= AUTO_TITLE_REFRESH_INTERVAL_MS {
        return Some(AutoTitleAction::Refresh);
    }

    if request
        .source_chars
        .saturating_sub(record.auto_title_source_chars)
        >= AUTO_TITLE_REFRESH_CHAR_THRESHOLD
    {
        return Some(AutoTitleAction::Refresh);
    }

    None
}

fn agent_session_title_request(
    record: &AgentSessionRecord,
    session_context: Option<&CLIAgentSessionContext>,
) -> Option<AgentSessionTitleRequest> {
    if record.title_overridden {
        return None;
    }

    let query = session_context.and_then(|context| title_source_text(context.query.as_deref()));
    let response =
        session_context.and_then(|context| title_source_text(context.response.as_deref()));
    let transcript = title_source_text(record.hosted_transcript.as_deref());
    let transcript_prompt = record
        .hosted_transcript
        .as_deref()
        .and_then(|transcript| first_user_prompt_from_hosted_transcript(transcript, record.agent))
        .and_then(|prompt| title_source_text(Some(&prompt)));
    let query = query.or(transcript_prompt);
    if query.is_none() && response.is_none() && transcript.is_none() {
        return None;
    }

    let summary = session_context.and_then(|context| title_source_text(context.summary.as_deref()));
    let first_prompt_title = query.as_deref().and_then(compact_session_title);
    let fallback_title = local_agent_session_title_from_parts(
        summary.as_deref(),
        query.as_deref(),
        response.as_deref(),
    )?;
    let project = friendly_path(&record.project_path);
    let source_chars = auto_title_source_chars(record, session_context);
    let source = format!(
        "agent={}\nproject={}\nsummary={}\nquery={}\nresponse={}\ntranscript={}",
        record.agent.display_name(),
        project,
        summary.as_deref().unwrap_or(""),
        query.as_deref().unwrap_or(""),
        response.as_deref().unwrap_or(""),
        transcript.as_deref().unwrap_or("")
    );
    let fingerprint = stable_hash_u64(&source);
    let prompt = format!(
        "Generate a concise sidebar title for this coding agent session.\n\
Return only the title text. Use 2-6 words. Do not use quotes, markdown, or trailing punctuation. \
Prefer the user's language when it is clear. Do not run tools or inspect files.\n\n\
Agent: {}\nProject: {}\n\n\
Latest user prompt:\n{}\n\n\
Latest agent response:\n{}\n\n\
Existing status summary:\n{}\n\n\
Recent terminal transcript tail:\n{}",
        record.agent.display_name(),
        project,
        query.as_deref().unwrap_or("(none)"),
        response.as_deref().unwrap_or("(none)"),
        summary.as_deref().unwrap_or("(none)"),
        transcript.as_deref().unwrap_or("(none)")
    );

    Some(AgentSessionTitleRequest {
        agent: record.agent,
        project_path: record.project_path.clone(),
        fingerprint,
        prompt,
        first_prompt_title,
        fallback_title,
        source_chars,
    })
}

fn first_prompt_session_title(session_context: &CLIAgentSessionContext) -> Option<String> {
    let query = title_source_text(session_context.query.as_deref());
    query.as_deref().and_then(compact_session_title)
}

fn first_user_prompt_from_hosted_transcript(transcript: &str, agent: CLIAgent) -> Option<String> {
    let mut next_nonempty_line_is_user_prompt = false;
    for line in transcript.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("User:") || line.eq_ignore_ascii_case("User") {
            next_nonempty_line_is_user_prompt = true;
            continue;
        }

        if next_nonempty_line_is_user_prompt {
            return Some(line.to_owned());
        }

        if agent == CLIAgent::Codex {
            if let Some(prompt) = line.strip_prefix('\u{203a}') {
                let prompt = prompt.trim();
                if !prompt.is_empty() {
                    return Some(prompt.to_owned());
                }
            }
        }
    }

    None
}

fn local_agent_session_title_from_parts(
    summary: Option<&str>,
    query: Option<&str>,
    response: Option<&str>,
) -> Option<String> {
    query
        .or(summary)
        .or(response)
        .and_then(compact_session_title)
}

fn auto_title_source_chars(
    record: &AgentSessionRecord,
    session_context: Option<&CLIAgentSessionContext>,
) -> usize {
    let context_chars = session_context
        .map(|context| {
            [
                context.summary.as_deref(),
                context.query.as_deref(),
                context.response.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(|text| text.chars().count())
            .sum::<usize>()
        })
        .unwrap_or_default();
    let transcript_chars = record
        .hosted_transcript
        .as_deref()
        .map(|text| text.chars().count())
        .unwrap_or_default();

    context_chars.max(transcript_chars)
}

fn title_source_text(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.replace('\r', " ").replace('\t', " "))
        .map(|text| text.lines().map(str::trim).collect::<Vec<_>>().join("\n"))
        .and_then(|text| {
            let text = text.trim();
            (!text.is_empty()).then(|| tail_truncate_chars(text, 2_000))
        })
}

fn compact_session_title(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let first_line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(text);
    let first_sentence = first_line
        .split(['.', '!', '?', '。', '！', '？', '\n'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(first_line);
    sanitize_generated_session_title(first_sentence)
}

fn sanitize_generated_session_title(title: &str) -> Option<String> {
    let mut title = title
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .trim_matches(|ch: char| {
            ch == '"'
                || ch == '\''
                || ch == '`'
                || ch == '*'
                || ch == '#'
                || ch == '-'
                || ch == ':'
                || ch.is_whitespace()
        })
        .to_owned();

    for prefix in ["Title:", "title:", "Session title:", "session title:"] {
        if let Some(stripped) = title.strip_prefix(prefix) {
            title = stripped.trim().to_owned();
            break;
        }
    }

    title = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| {
            ch == '"'
                || ch == '\''
                || ch == '`'
                || ch == '*'
                || ch == '#'
                || ch == '-'
                || ch == ':'
                || ch == '.'
                || ch == ';'
                || ch.is_whitespace()
        })
        .to_owned();

    if title.is_empty() {
        return None;
    }

    let mut chars = title.chars();
    let truncated = chars
        .by_ref()
        .take(MAX_AUTO_TITLE_CHARS)
        .collect::<String>();
    let title = if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    };

    Some(truncate_title(title))
}

async fn generate_title_with_matching_agent(
    agent: CLIAgent,
    project_path: &Path,
    prompt: &str,
) -> Option<String> {
    let output = run_matching_agent_title_command(agent, project_path, prompt).await?;
    sanitize_generated_session_title(&output)
}

#[cfg(not(target_family = "wasm"))]
async fn run_matching_agent_title_command(
    agent: CLIAgent,
    project_path: &Path,
    prompt: &str,
) -> Option<String> {
    let mut command = command::r#async::Command::new(agent.command_prefix());
    match agent {
        CLIAgent::Codex => {
            command.args([
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--ignore-rules",
                "--sandbox",
                "read-only",
                "--ask-for-approval",
                "never",
                "--color",
                "never",
                "-C",
            ]);
            command.arg(project_path);
            command.arg(prompt);
        }
        CLIAgent::Claude => {
            command.args(["-p", prompt, "--output-format", "text"]);
            command.current_dir(project_path);
        }
        CLIAgent::OpenCode => {
            command.args(["--prompt", prompt]);
            command.current_dir(project_path);
        }
        _ => return None,
    }

    command
        .stdin(command::Stdio::null())
        .stdout(command::Stdio::piped())
        .stderr(command::Stdio::piped())
        .kill_on_drop(true);

    let output = command.output();
    let timeout = Timer::after(AUTO_TITLE_CLI_TIMEOUT);
    futures::pin_mut!(output);
    futures::pin_mut!(timeout);

    let output = match futures::future::select(output, timeout).await {
        futures::future::Either::Left((Ok(output), _)) => output,
        futures::future::Either::Left((Err(err), _)) => {
            log::debug!(
                "Failed to run {} for agent session title: {err}",
                agent.display_name()
            );
            return None;
        }
        futures::future::Either::Right((_, _)) => {
            log::debug!(
                "{} agent session title generation timed out",
                agent.display_name()
            );
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "{} agent session title generation failed: {}",
            agent.display_name(),
            stderr.trim()
        );
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

#[cfg(target_family = "wasm")]
async fn run_matching_agent_title_command(
    _agent: CLIAgent,
    _project_path: &Path,
    _prompt: &str,
) -> Option<String> {
    None
}

fn stable_hash_u64(text: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn project_path_from_session_context(session_context: &CLIAgentSessionContext) -> Option<PathBuf> {
    session_context
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(PathBuf::from)
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

#[cfg(not(target_family = "wasm"))]
fn codex_home_dir() -> Option<PathBuf> {
    if let Some(codex_home) = std::env::var_os("CODEX_HOME") {
        return Some(PathBuf::from(codex_home));
    }

    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex"))
}

#[cfg(not(target_family = "wasm"))]
fn latest_codex_session_id_for_project(project_path: &Path) -> Option<String> {
    let codex_home = codex_home_dir()?;
    latest_codex_session_id_for_project_in_home(&codex_home, project_path)
}

#[cfg(target_family = "wasm")]
fn latest_codex_session_id_for_project(_project_path: &Path) -> Option<String> {
    None
}

#[cfg(not(target_family = "wasm"))]
fn latest_codex_session_id_for_project_in_home(
    codex_home: &Path,
    project_path: &Path,
) -> Option<String> {
    let sessions_dir = codex_home.join("sessions");
    let project_path = normalized_path_key(project_path);
    let mut best: Option<(u8, SystemTime, String)> = None;

    visit_codex_session_files(&sessions_dir, &project_path, &mut best);
    best.map(|(_, _, session_id)| session_id)
}

#[derive(Debug, Clone)]
struct CodexDiscoveredSession {
    id: String,
    parent_agent_session_id: String,
    title: String,
    modified_at_ms: i64,
}

#[cfg(not(target_family = "wasm"))]
fn codex_child_sessions_for_project(
    project_path: &Path,
    parent_session_ids: &BTreeSet<String>,
) -> Vec<CodexDiscoveredSession> {
    let Some(codex_home) = codex_home_dir() else {
        return Vec::new();
    };
    codex_child_sessions_for_project_in_home(&codex_home, project_path, parent_session_ids)
}

#[cfg(target_family = "wasm")]
fn codex_child_sessions_for_project(
    _project_path: &Path,
    _parent_session_ids: &BTreeSet<String>,
) -> Vec<CodexDiscoveredSession> {
    Vec::new()
}

#[cfg(not(target_family = "wasm"))]
fn codex_child_sessions_for_project_in_home(
    codex_home: &Path,
    project_path: &Path,
    parent_session_ids: &BTreeSet<String>,
) -> Vec<CodexDiscoveredSession> {
    let sessions_dir = codex_home.join("sessions");
    let project_path = normalized_path_key(project_path);
    let mut sessions = Vec::new();

    visit_codex_child_session_files(
        &sessions_dir,
        &project_path,
        parent_session_ids,
        &mut sessions,
    );
    sessions.sort_by(|a, b| b.modified_at_ms.cmp(&a.modified_at_ms));
    sessions
}

#[cfg(not(target_family = "wasm"))]
fn visit_codex_session_files(
    dir: &Path,
    project_path: &Path,
    best: &mut Option<(u8, SystemTime, String)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_codex_session_files(&path, project_path, best);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let Some(meta) = read_codex_session_meta(&path) else {
            continue;
        };
        let session_cwd = normalized_path_key(&meta.cwd);
        let Some(match_score) = codex_session_project_match_score(&session_cwd, project_path)
        else {
            continue;
        };

        let modified_at = path
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let should_replace = best
            .as_ref()
            .is_none_or(|(best_match_score, best_modified_at, _)| {
                match_score > *best_match_score
                    || (match_score == *best_match_score && modified_at > *best_modified_at)
            });
        if should_replace {
            *best = Some((match_score, modified_at, meta.id));
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn codex_session_project_match_score(session_cwd: &Path, project_path: &Path) -> Option<u8> {
    if session_cwd == project_path {
        Some(3)
    } else if project_path.starts_with(session_cwd) {
        Some(2)
    } else if session_cwd.starts_with(project_path) {
        Some(1)
    } else {
        None
    }
}

#[cfg(not(target_family = "wasm"))]
fn visit_codex_child_session_files(
    dir: &Path,
    project_path: &Path,
    parent_session_ids: &BTreeSet<String>,
    sessions: &mut Vec<CodexDiscoveredSession>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_codex_child_session_files(&path, project_path, parent_session_ids, sessions);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let Some(meta) = read_codex_session_meta(&path) else {
            continue;
        };
        if normalized_path_key(&meta.cwd) != project_path {
            continue;
        }
        let Some(parent_agent_session_id) = meta.parent_thread_id.clone() else {
            continue;
        };
        if !parent_session_ids.contains(&parent_agent_session_id) {
            continue;
        }

        let modified_at = path
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let title = meta.child_title();
        sessions.push(CodexDiscoveredSession {
            id: meta.id,
            parent_agent_session_id,
            title,
            modified_at_ms: system_time_to_ms(modified_at),
        });
    }
}

#[cfg(not(target_family = "wasm"))]
fn read_codex_session_meta(path: &Path) -> Option<CodexSessionMeta> {
    let file = File::open(path).ok()?;
    let mut lines = BufReader::new(file).lines();
    let first_line = lines.next()?.ok()?;
    let meta = serde_json::from_str::<CodexSessionMetaLine>(&first_line).ok()?;

    if meta.kind != "session_meta" {
        return None;
    }

    Some(meta.payload)
}

#[cfg(not(target_family = "wasm"))]
fn normalized_path_key(path: &Path) -> PathBuf {
    let expanded = expand_tilde(path);
    expanded.canonicalize().unwrap_or(expanded)
}

#[cfg(not(target_family = "wasm"))]
fn expand_tilde(path: &Path) -> PathBuf {
    let path_text = path.to_string_lossy();
    if path_text == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = path_text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    path.to_path_buf()
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct CodexSessionMetaLine {
    #[serde(rename = "type")]
    kind: String,
    payload: CodexSessionMeta,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct CodexSessionMeta {
    id: String,
    cwd: PathBuf,
    #[serde(default)]
    parent_thread_id: Option<String>,
    #[serde(default)]
    agent_nickname: Option<String>,
}

#[cfg(not(target_family = "wasm"))]
impl CodexSessionMeta {
    fn child_title(&self) -> String {
        self.agent_nickname
            .as_deref()
            .map(str::trim)
            .filter(|nickname| !nickname.is_empty())
            .map(|nickname| format!("Codex child - {nickname}"))
            .unwrap_or_else(|| "Codex child session".to_owned())
    }
}

#[cfg(not(target_family = "wasm"))]
fn system_time_to_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_else(now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_project_keys_moves_before_and_after() {
        let order = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

        assert_eq!(
            reorder_project_keys(&order, "c", "a", AgentProjectMovePlacement::Before),
            Some(vec!["c".to_owned(), "a".to_owned(), "b".to_owned()])
        );
        assert_eq!(
            reorder_project_keys(&order, "a", "c", AgentProjectMovePlacement::After),
            Some(vec!["b".to_owned(), "c".to_owned(), "a".to_owned()])
        );
    }

    #[test]
    fn reorder_project_keys_ignores_noop_or_unknown_keys() {
        let order = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

        assert_eq!(
            reorder_project_keys(&order, "a", "a", AgentProjectMovePlacement::Before),
            None
        );
        assert_eq!(
            reorder_project_keys(&order, "missing", "a", AgentProjectMovePlacement::Before),
            None
        );
        assert_eq!(
            reorder_project_keys(&order, "a", "missing", AgentProjectMovePlacement::After),
            None
        );
    }

    #[test]
    fn persisted_record_skips_terminal_view_id() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            environment_id: default_agent_session_environment_id(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Codex,
            title: "Fix parser".to_owned(),
            status: AgentSessionStatus::InProgress,
            agent_session_id: Some("agent-session".to_owned()),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: 10,
            sort_order: 10,
            is_pinned: true,
            archived_at_ms: Some(11),
            title_overridden: true,
            auto_title_fingerprint: Some(123),
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: Some("User:\nhello\n\nAgent:\nhi\n\n".to_owned()),
            hosted_transcript_updated_at_ms: Some(12),
            terminal_view_id: None,
            group_terminal_view_id: None,
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
        assert_eq!(restored.auto_title_fingerprint, Some(123));
        assert_eq!(
            restored.hosted_transcript.as_deref(),
            Some("User:\nhello\n\nAgent:\nhi\n\n")
        );
        assert_eq!(restored.hosted_transcript_updated_at_ms, Some(12));
        assert_eq!(restored.parent_session_id, None);
        assert_eq!(restored.parent_agent_session_id, None);
        assert_eq!(restored.sort_order, 10);
        assert_eq!(restored.terminal_view_id, None);
        assert_eq!(restored.group_terminal_view_id, None);
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
        assert_eq!(restored.auto_title_fingerprint, None);
        assert_eq!(restored.auto_title_summarized_at_ms, None);
        assert_eq!(restored.auto_title_source_chars, 0);
        assert_eq!(restored.hosted_transcript, None);
        assert_eq!(restored.hosted_transcript_updated_at_ms, None);
        assert_eq!(restored.parent_session_id, None);
        assert_eq!(restored.parent_agent_session_id, None);
        assert_eq!(restored.sort_order, 0);
        assert_eq!(restored.group_terminal_view_id, None);
    }

    #[test]
    fn hosted_transcript_is_trimmed_and_wrapped_for_restore() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            environment_id: default_agent_session_environment_id(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Claude,
            title: "Fix parser".to_owned(),
            status: AgentSessionStatus::Success,
            agent_session_id: None,
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: 10,
            sort_order: 10,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            auto_title_fingerprint: None,
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: normalize_hosted_transcript("  User:\nhello\n\n  ".to_owned()),
            hosted_transcript_updated_at_ms: Some(12),
            terminal_view_id: None,
            group_terminal_view_id: None,
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
                environment_id: default_agent_session_environment_id(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "Old pinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                parent_session_id: None,
                parent_agent_session_id: None,
                updated_at_ms: 1,
                sort_order: 1,
                is_pinned: true,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            },
            AgentSessionRecord {
                id: "new-unpinned".to_owned(),
                environment_id: default_agent_session_environment_id(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "New unpinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                parent_session_id: None,
                parent_agent_session_id: None,
                updated_at_ms: 20,
                sort_order: 20,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            },
            AgentSessionRecord {
                id: "new-pinned".to_owned(),
                environment_id: default_agent_session_environment_id(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: "New pinned".to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                parent_session_id: None,
                parent_agent_session_id: None,
                updated_at_ms: 10,
                sort_order: 10,
                is_pinned: true,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
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

    #[test]
    fn generated_session_title_is_sanitized_and_truncated() {
        assert_eq!(
            sanitize_generated_session_title("Title: \"Review current changes.\""),
            Some("Review current changes".to_owned())
        );
        assert_eq!(sanitize_generated_session_title("   "), None);
        assert!(
            sanitize_generated_session_title(
                "A very long generated title that should be shortened for the sidebar"
            )
            .unwrap()
            .len()
                <= MAX_AUTO_TITLE_CHARS + 3
        );
    }

    #[test]
    fn agent_session_title_request_uses_first_prompt_fallback() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            environment_id: default_agent_session_environment_id(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Codex,
            title: "New Codex session".to_owned(),
            status: AgentSessionStatus::Success,
            agent_session_id: Some("agent-session".to_owned()),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: 10,
            sort_order: 10,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            auto_title_fingerprint: None,
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: None,
            hosted_transcript_updated_at_ms: None,
            terminal_view_id: None,
            group_terminal_view_id: None,
        };
        let context = CLIAgentSessionContext {
            summary: Some("Review current changes".to_owned()),
            query: Some("Run /review on my current changes".to_owned()),
            response: Some("I found two test gaps.".to_owned()),
            ..Default::default()
        };

        let request = agent_session_title_request(&record, Some(&context)).unwrap();

        assert_eq!(
            request.first_prompt_title.as_deref(),
            Some("Run /review on my current changes")
        );
        assert_eq!(request.fallback_title, "Run /review on my current changes");
        assert!(request.prompt.contains("Latest user prompt"));
        assert_ne!(request.fingerprint, 0);
    }

    #[test]
    fn auto_title_action_uses_first_prompt_before_ai_refresh() {
        let record = AgentSessionRecord {
            id: "record-1".to_owned(),
            environment_id: default_agent_session_environment_id(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Codex,
            title: "New Codex session".to_owned(),
            status: AgentSessionStatus::Success,
            agent_session_id: Some("agent-session".to_owned()),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: 10,
            sort_order: 10,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            auto_title_fingerprint: None,
            auto_title_summarized_at_ms: None,
            auto_title_source_chars: 0,
            hosted_transcript: None,
            hosted_transcript_updated_at_ms: None,
            terminal_view_id: None,
            group_terminal_view_id: None,
        };
        let context = CLIAgentSessionContext {
            query: Some("Explain this codebase. Then write notes.".to_owned()),
            response: Some("Sure.".to_owned()),
            ..Default::default()
        };
        let request = agent_session_title_request(&record, Some(&context)).unwrap();

        assert_eq!(
            request.first_prompt_title.as_deref(),
            Some("Explain this codebase")
        );
        assert_eq!(
            auto_title_action(&record, &request, 1_000),
            Some(AutoTitleAction::FirstPrompt)
        );
    }

    #[test]
    fn first_user_prompt_from_hosted_transcript_reads_codex_prompt_line() {
        let transcript = "OpenAI Codex\n\n\u{203a} hello\n\n\u{2022} Hi there";

        assert_eq!(
            first_user_prompt_from_hosted_transcript(transcript, CLIAgent::Codex).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn first_user_prompt_from_hosted_transcript_reads_user_section() {
        let transcript = "User:\nExplain this codebase\n\nAgent:\nSure";

        assert_eq!(
            first_user_prompt_from_hosted_transcript(transcript, CLIAgent::Claude).as_deref(),
            Some("Explain this codebase")
        );
    }

    #[test]
    fn auto_title_action_refreshes_after_interval_or_source_threshold() {
        let mut record = AgentSessionRecord {
            id: "record-1".to_owned(),
            environment_id: default_agent_session_environment_id(),
            project_path: PathBuf::from("/tmp/project"),
            agent: CLIAgent::Codex,
            title: "Explain this codebase".to_owned(),
            status: AgentSessionStatus::Success,
            agent_session_id: Some("agent-session".to_owned()),
            parent_session_id: None,
            parent_agent_session_id: None,
            updated_at_ms: 10,
            sort_order: 10,
            is_pinned: false,
            archived_at_ms: None,
            title_overridden: false,
            auto_title_fingerprint: Some(1),
            auto_title_summarized_at_ms: Some(1_000),
            auto_title_source_chars: 100,
            hosted_transcript: None,
            hosted_transcript_updated_at_ms: None,
            terminal_view_id: None,
            group_terminal_view_id: None,
        };
        let mut request = AgentSessionTitleRequest {
            agent: CLIAgent::Codex,
            project_path: PathBuf::from("/tmp/project"),
            fingerprint: 2,
            prompt: "title me".to_owned(),
            first_prompt_title: Some("Explain this codebase".to_owned()),
            fallback_title: "Explain this codebase".to_owned(),
            source_chars: 200,
        };

        assert_eq!(
            auto_title_action(
                &record,
                &request,
                1_000 + AUTO_TITLE_REFRESH_INTERVAL_MS - 1
            ),
            None
        );
        assert_eq!(
            auto_title_action(&record, &request, 1_000 + AUTO_TITLE_REFRESH_INTERVAL_MS),
            Some(AutoTitleAction::Refresh)
        );

        request.source_chars = record.auto_title_source_chars + AUTO_TITLE_REFRESH_CHAR_THRESHOLD;
        assert_eq!(
            auto_title_action(&record, &request, 1_500),
            Some(AutoTitleAction::Refresh)
        );

        record.title_overridden = true;
        assert_eq!(auto_title_action(&record, &request, 1_500), None);
    }

    #[test]
    fn terminal_view_ids_for_deleted_session_includes_group_children_and_dedupes() {
        fn record(
            id: &str,
            parent_session_id: Option<&str>,
            terminal_view_id: Option<EntityId>,
            group_terminal_view_id: Option<EntityId>,
        ) -> AgentSessionRecord {
            AgentSessionRecord {
                id: id.to_owned(),
                environment_id: default_agent_session_environment_id(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: id.to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: None,
                parent_session_id: parent_session_id.map(str::to_owned),
                parent_agent_session_id: None,
                updated_at_ms: 1,
                sort_order: 1,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id,
                group_terminal_view_id,
            }
        }

        let group_terminal = EntityId::new();
        let child_terminal = EntityId::new();
        let unrelated_terminal = EntityId::new();
        let model = AgentSessionsModel {
            pending_title_generations: HashSet::new(),
            project_paths: Vec::new(),
            records: vec![
                record("parent", None, Some(group_terminal), Some(group_terminal)),
                record("child", Some("parent"), Some(child_terminal), None),
                record("unrelated", None, Some(unrelated_terminal), None),
            ],
        };

        let deleted_group_terminal_ids = model.terminal_view_ids_for_deleted_session("parent");
        assert_eq!(deleted_group_terminal_ids.len(), 2);
        assert!(deleted_group_terminal_ids.contains(&group_terminal));
        assert!(deleted_group_terminal_ids.contains(&child_terminal));
        assert!(!deleted_group_terminal_ids.contains(&unrelated_terminal));

        let deleted_child_terminal_ids = model.terminal_view_ids_for_deleted_session("child");
        assert_eq!(deleted_child_terminal_ids, vec![child_terminal]);

        let parent_terminal = EntityId::new();
        let group_shell_terminal = EntityId::new();
        let model = AgentSessionsModel {
            pending_title_generations: HashSet::new(),
            project_paths: Vec::new(),
            records: vec![
                record(
                    "parent",
                    None,
                    Some(parent_terminal),
                    Some(group_shell_terminal),
                ),
                record("child", Some("parent"), Some(child_terminal), None),
            ],
        };

        let disbanded_terminal_ids = model.terminal_view_ids_for_disbanded_group("parent");
        assert_eq!(disbanded_terminal_ids.len(), 3);
        assert!(disbanded_terminal_ids.contains(&parent_terminal));
        assert!(disbanded_terminal_ids.contains(&group_shell_terminal));
        assert!(disbanded_terminal_ids.contains(&child_terminal));
    }

    #[test]
    fn clears_duplicate_codex_session_ids() {
        fn record(
            id: &str,
            project_path: &Path,
            agent_session_id: Option<&str>,
            parent_session_id: Option<&str>,
        ) -> AgentSessionRecord {
            AgentSessionRecord {
                id: id.to_owned(),
                environment_id: default_agent_session_environment_id(),
                project_path: project_path.to_path_buf(),
                agent: CLIAgent::Codex,
                title: id.to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: agent_session_id.map(str::to_owned),
                parent_session_id: parent_session_id.map(str::to_owned),
                parent_agent_session_id: None,
                updated_at_ms: 1,
                sort_order: 1,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            }
        }

        let project_path = PathBuf::from("/tmp/project");
        let mut model = AgentSessionsModel {
            pending_title_generations: HashSet::new(),
            project_paths: Vec::new(),
            records: vec![
                record("parent", &project_path, Some("shared-codex-id"), None),
                record(
                    "child-1",
                    &project_path,
                    Some("shared-codex-id"),
                    Some("parent"),
                ),
                record(
                    "child-2",
                    &project_path,
                    Some("shared-codex-id"),
                    Some("parent"),
                ),
            ],
        };

        assert!(model.clear_duplicate_agent_session_ids_for_project(&project_path));
        assert_eq!(
            model.session("parent").unwrap().agent_session_id.as_deref(),
            Some("shared-codex-id")
        );
        assert_eq!(model.session("child-1").unwrap().agent_session_id, None);
        assert_eq!(model.session("child-2").unwrap().agent_session_id, None);
    }

    #[test]
    fn clears_duplicate_codex_session_ids_between_root_sessions() {
        fn record(id: &str, updated_at_ms: i64) -> AgentSessionRecord {
            AgentSessionRecord {
                id: id.to_owned(),
                environment_id: default_agent_session_environment_id(),
                project_path: PathBuf::from("/tmp/project"),
                agent: CLIAgent::Codex,
                title: id.to_owned(),
                status: AgentSessionStatus::InProgress,
                agent_session_id: Some("shared-codex-id".to_owned()),
                parent_session_id: None,
                parent_agent_session_id: None,
                updated_at_ms,
                sort_order: updated_at_ms,
                is_pinned: false,
                archived_at_ms: None,
                title_overridden: false,
                auto_title_fingerprint: None,
                auto_title_summarized_at_ms: None,
                auto_title_source_chars: 0,
                hosted_transcript: None,
                hosted_transcript_updated_at_ms: None,
                terminal_view_id: None,
                group_terminal_view_id: None,
            }
        }

        let project_path = PathBuf::from("/tmp/project");
        let mut model = AgentSessionsModel {
            pending_title_generations: HashSet::new(),
            project_paths: Vec::new(),
            records: vec![record("old-root", 1), record("new-root", 10)],
        };

        assert!(model.clear_duplicate_agent_session_ids_for_project(&project_path));
        assert_eq!(model.session("old-root").unwrap().agent_session_id, None);
        assert_eq!(
            model
                .session("new-root")
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("shared-codex-id")
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn finds_latest_codex_session_id_for_project() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let sessions_dir = temp_dir.path().join("sessions/2026/06/04");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        std::fs::write(
            sessions_dir.join("old.jsonl"),
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"old-session","cwd":{}}}}}"#,
                serde_json::to_string(&project_dir).unwrap()
            ),
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("other.jsonl"),
            r#"{"type":"session_meta","payload":{"id":"other-session","cwd":"/tmp/other"}}"#,
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(
            sessions_dir.join("new.jsonl"),
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"new-session","cwd":{}}}}}"#,
                serde_json::to_string(&project_dir).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            latest_codex_session_id_for_project_in_home(temp_dir.path(), &project_dir).as_deref(),
            Some("new-session")
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn finds_parent_codex_session_id_for_project_subdirectory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join("project");
        let nested_dir = project_dir.join("nested");
        std::fs::create_dir_all(&nested_dir).unwrap();

        let sessions_dir = temp_dir.path().join("sessions/2026/06/04");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        std::fs::write(
            sessions_dir.join("parent.jsonl"),
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"parent-session","cwd":{}}}}}"#,
                serde_json::to_string(&project_dir).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            latest_codex_session_id_for_project_in_home(temp_dir.path(), &nested_dir).as_deref(),
            Some("parent-session")
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn finds_codex_child_sessions_for_parent_project() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let sessions_dir = temp_dir.path().join("sessions/2026/06/04");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        std::fs::write(
            sessions_dir.join("child.jsonl"),
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"child-session","cwd":{},"parent_thread_id":"parent-session","agent_nickname":"Carson"}}}}"#,
                serde_json::to_string(&project_dir).unwrap()
            ),
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("other-parent.jsonl"),
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"other-parent-child","cwd":{},"parent_thread_id":"other-parent"}}}}"#,
                serde_json::to_string(&project_dir).unwrap()
            ),
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("other-project.jsonl"),
            r#"{"type":"session_meta","payload":{"id":"other-project-child","cwd":"/tmp/other","parent_thread_id":"parent-session"}}"#,
        )
        .unwrap();

        let parent_session_ids = BTreeSet::from(["parent-session".to_owned()]);
        let child_sessions = codex_child_sessions_for_project_in_home(
            temp_dir.path(),
            &project_dir,
            &parent_session_ids,
        );

        assert_eq!(child_sessions.len(), 1);
        assert_eq!(child_sessions[0].id, "child-session");
        assert_eq!(child_sessions[0].parent_agent_session_id, "parent-session");
        assert_eq!(child_sessions[0].title, "Codex child - Carson");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionLifecycle {
    /// `terminal_view_id` is None — the session was never attached in this
    /// process, or its `#[serde(skip)]` binding was lost across a restart.
    NotStarted,
    /// The terminal's active block is still in `Executing` (or `Background`).
    /// The agent is alive: clicking the row must just focus, never relaunch.
    Running,
    /// The terminal exists but its block has finished
    /// (`DoneWithExecution` / `DoneWithNoExecution` / `Static` / no active
    /// block). Typical cause: the user `Ctrl+D`'d out of codex, or the agent
    /// crashed. Relaunching matches the user's click intent.
    Dead,
}

/// Snapshot the session's lifecycle at render time so the click handler can
/// decide between `FocusAgentSession` (running) and `RestoreAgentSession`
/// (needs to (re)launch) without re-inspecting the model on the event path.
///
/// Why not just use `record.terminal_view_id.is_some()`? Because that field
/// stays `Some` even after the agent inside the terminal exits (e.g. `Ctrl+D`
/// from a codex TUI). A user who kills the agent and then clicks the row
/// expects Warp to bring the session back, not silently refocus a dead tab.
///
/// Why not `is_long_running`? It is false for the first ~1s after launch
/// (the `LONG_RUNNING_COMMAND_DURATION_MS` cache hasn't kicked in yet) and
/// also false once the block finishes. We want neither of those to be
/// mis-classified as "dead", so we read the block state directly.
fn compute_session_lifecycle_state(
    record: &AgentSessionRecord,
    app: &AppContext,
) -> SessionLifecycle {
    let Some(terminal_view_id) = record.terminal_view_id else {
        return SessionLifecycle::NotStarted;
    };

    // The session may have been opened in another window. Scan every window
    // we know about; if any of them still hosts a running block, treat the
    // session as alive. We do not need to mutate any view, only peek, so
    // walking windows here is safe.
    for window_id in app.window_ids() {
        let Some(handle) = app.view_with_id::<TerminalView>(window_id, terminal_view_id) else {
            continue;
        };
        let terminal_view = handle.as_ref(app);
        // Hold the lock only for the brief active-block read; this matches
        // the existing pattern in workspace/view.rs and never escapes this
        // function, so it cannot deadlock with model acquisition elsewhere.
        let model = terminal_view.model.lock();
        if matches!(
            model.block_list().active_block().state(),
            BlockState::Executing | BlockState::Background
        ) {
            return SessionLifecycle::Running;
        }
        return SessionLifecycle::Dead;
    }

    // `terminal_view_id` is set but the TerminalView is gone from every
    // window. The view was closed or the app reloaded; treat as not started
    // so the next click reopens the session.
    SessionLifecycle::NotStarted
}
