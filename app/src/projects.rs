use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;

use chrono::Utc;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::persistence::model::Project;
use crate::persistence::ModelEvent;

#[derive(Debug)]
pub enum ProjectEvent {
    Added {
        #[expect(unused, reason = "TODO(jparker): #pod-code-mode wip")]
        path: PathBuf,
    },
    #[expect(unused, reason = "TODO(jparker): #pod-code-mode wip")]
    Removed { path: PathBuf },
    #[expect(unused, reason = "TODO(jparker): #pod-code-mode wip")]
    Updated { path: PathBuf },
}

pub struct ProjectManagementModel {
    projects: HashMap<PathBuf, Project>,
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl Entity for ProjectManagementModel {
    type Event = ProjectEvent;
}

impl SingletonEntity for ProjectManagementModel {}

impl ProjectManagementModel {
    /// Create a new Projects model with persisted data
    pub fn new(
        persisted_projects: Vec<Project>,
        model_event_sender: Option<SyncSender<ModelEvent>>,
        _ctx: &mut ModelContext<Self>,
    ) -> Self {
        log::debug!("Loading {} persisted projects", persisted_projects.len());

        let projects = persisted_projects
            .into_iter()
            .map(|project| (PathBuf::from(&project.path), project))
            .collect();

        Self {
            projects,
            model_event_sender,
        }
    }

    /// Add a project to the list. If it already exists, update the last_opened_ts.
    pub fn upsert_project(&mut self, path: PathBuf, ctx: &mut ModelContext<Self>) {
        let now = Utc::now().naive_utc();

        let project = if let Some(existing_project) = self.projects.get_mut(&path) {
            // Update existing project's last opened time
            existing_project.last_opened_ts = Some(now);
            existing_project.clone()
        } else {
            // Create new project
            let project = Project {
                path: path.to_string_lossy().to_string(),
                added_ts: now,
                last_opened_ts: Some(now),
            };
            self.projects.insert(path.clone(), project.clone());
            project
        };
        self.save_project(project);
        ctx.emit(ProjectEvent::Added { path });
    }

    pub fn remove_project(&mut self, path: &Path, ctx: &mut ModelContext<Self>) {
        let path = path.to_path_buf();
        if self.projects.remove(&path).is_none() {
            return;
        }

        self.delete_project(path.to_string_lossy().to_string());
        ctx.emit(ProjectEvent::Removed { path });
    }

    pub fn rename_project(
        &mut self,
        old_path: &Path,
        new_path: PathBuf,
        ctx: &mut ModelContext<Self>,
    ) {
        if old_path == new_path.as_path() {
            self.upsert_project(new_path, ctx);
            return;
        }

        let now = Utc::now().naive_utc();
        let mut project = self.projects.remove(old_path).unwrap_or(Project {
            path: new_path.to_string_lossy().to_string(),
            added_ts: now,
            last_opened_ts: None,
        });
        project.path = new_path.to_string_lossy().to_string();
        project.last_opened_ts = Some(now);

        self.delete_project(old_path.to_string_lossy().to_string());
        self.projects.insert(new_path.clone(), project.clone());
        self.save_project(project);
        ctx.emit(ProjectEvent::Updated { path: new_path });
    }

    pub fn all_projects(&self) -> impl Iterator<Item = &Project> {
        self.projects.values()
    }

    /// Save a project to the database
    fn save_project(&self, project: Project) {
        if let Some(sender) = &self.model_event_sender {
            let event = ModelEvent::UpsertProject { project };
            if let Err(err) = sender.send(event) {
                log::error!("Failed to save project to database: {err}");
            }
        }
    }

    fn delete_project(&self, path: String) {
        if let Some(sender) = &self.model_event_sender {
            let event = ModelEvent::DeleteProject { path };
            if let Err(err) = sender.send(event) {
                log::error!("Failed to delete project from database: {err}");
            }
        }
    }
}
