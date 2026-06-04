use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::ui::Icon;
use warp_core::user_preferences::GetUserPreferences as _;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Element, Fill as ElementFill, Flex, Hoverable, MainAxisAlignment,
    MainAxisSize, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Shrinkable, Text, Wrap,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity, View, ViewContext};

use crate::appearance::Appearance;
use crate::projects::ProjectManagementModel;
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionStatus, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::CLIAgent;
use crate::workspace::WorkspaceAction;

const AGENT_SESSION_RECORDS_PREF_KEY: &str = "agent_sessions.records.v1";
const MAX_AGENT_SESSION_RECORDS: usize = 200;
const MAX_TITLE_CHARS: usize = 96;

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
    #[serde(skip, default)]
    pub terminal_view_id: Option<EntityId>,
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
                if let Some(title) = session_context.display_title() {
                    record.title = truncate_title(title);
                }
                if let Some(session_id) = &session_context.session_id {
                    record.agent_session_id = Some(session_id.clone());
                }
            }),
            CLIAgentSessionsModelEvent::SessionUpdated { agent, .. } => {
                let session = CLIAgentSessionsModel::as_ref(ctx)
                    .session(terminal_view_id)
                    .cloned();
                self.update_record_for_terminal(terminal_view_id, |record| {
                    record.agent = *agent;
                    if let Some(session) = session {
                        record.status = AgentSessionStatus::from_cli_status(&session.status);
                        if let Some(title) = session.session_context.display_title() {
                            record.title = truncate_title(title);
                        }
                        if let Some(session_id) = session.session_context.session_id {
                            record.agent_session_id = Some(session_id);
                        }
                    }
                })
            }
            CLIAgentSessionsModelEvent::InputSessionChanged { .. } => false,
            CLIAgentSessionsModelEvent::Ended { .. } => false,
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

        Self {
            scroll_state: ClippedScrollStateHandle::default(),
            row_mouse_states: RefCell::new(HashMap::new()),
            add_project_mouse_state: MouseStateHandle::default(),
            empty_state_mouse_state: MouseStateHandle::default(),
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

    fn render_add_project_button(&self, app: &AppContext) -> Box<dyn Element> {
        render_compact_button(
            self.add_project_mouse_state.clone(),
            Icon::Plus,
            "Project",
            app,
            WorkspaceAction::OpenAgentSessionProjectPicker,
        )
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
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Flex::column()
                        .with_spacing(2.)
                        .with_child(
                            Text::new_inline(project_name, font_family.clone(), 13.)
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .with_child(
                            Text::new_inline(project_path_text, font_family.clone(), 11.)
                                .with_color(theme.sub_text_color(theme.background()).into())
                                .finish(),
                        )
                        .finish(),
                )
                .finish(),
            )
            .finish();

        let mut project_column = Flex::column().with_spacing(8.).with_child(header);

        let agent_row = Wrap::row()
            .with_spacing(6.)
            .with_run_spacing(6.)
            .with_children(
                SUPPORTED_AGENTS
                    .into_iter()
                    .map(|agent| self.render_agent_chip(project_path, agent, app)),
            )
            .finish();
        project_column.add_child(agent_row);

        if sessions.is_empty() {
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
            for session in sessions {
                project_column.add_child(self.render_session_row(session, app));
            }
        }

        Container::new(project_column.finish())
            .with_horizontal_padding(12.)
            .with_vertical_padding(10.)
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
        let label = agent.display_name();
        let icon = agent.icon().unwrap_or(Icon::Terminal);

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
        let title = session.title.clone();
        let meta = format!(
            "{} - {}",
            session.agent.display_name(),
            session.status.label()
        );
        let status_fill = status_fill(session.status, app);
        let icon = session.agent.icon().unwrap_or(Icon::Terminal);

        Hoverable::new(mouse_state, move |state| {
            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
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
                                .with_child(
                                    Text::new_inline(title.clone(), font_family.clone(), 12.)
                                        .with_color(
                                            theme.main_text_color(theme.background()).into(),
                                        )
                                        .finish(),
                                )
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

            if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }
            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(WorkspaceAction::RestoreAgentSession {
                session_id: session_id.clone(),
            });
        })
        .finish()
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
        let mut content = Flex::column().with_spacing(2.).with_child(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::End)
                    .with_child(self.render_add_project_button(app))
                    .finish(),
            )
            .with_horizontal_padding(12.)
            .with_vertical_padding(8.)
            .finish(),
        );

        for project_path in projects {
            let mut sessions = records
                .records()
                .iter()
                .filter(|record| record.project_path == project_path)
                .cloned()
                .collect::<Vec<_>>();
            sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
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
            terminal_view_id: None,
        };

        let serialized = serde_json::to_string(&record).unwrap();
        assert!(!serialized.contains("terminal_view_id"));

        let restored = serde_json::from_str::<AgentSessionRecord>(&serialized).unwrap();
        assert_eq!(restored.id, "record-1");
        assert_eq!(restored.agent, CLIAgent::Codex);
        assert_eq!(restored.terminal_view_id, None);
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
