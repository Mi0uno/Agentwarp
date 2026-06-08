pub mod auth_manager;
mod auth_override_warning_body;
pub mod auth_override_warning_modal;
mod auth_view_body;
pub mod auth_view_modal;
mod auth_view_shared_helpers;
mod login_error_modal;
mod login_failure_notification;
pub mod login_slide;
pub mod needs_sso_link_view;
pub mod paste_auth_token_modal;
mod user_properties;
pub use warp_server_auth::{auth_state, credentials, user, user_uid};
#[cfg(target_family = "wasm")]
pub mod web_handoff;

use ::settings::SettingsManager;
use ai::index::full_source_code_embedding::manager::CodebaseIndexManager;
pub use auth_manager::AuthManager;
pub use auth_state::AuthStateProvider;
use itertools::Itertools;
pub use login_failure_notification::LoginFailureReason;
pub use user_uid::UserUid;
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::{AppContext, SingletonEntity};

use crate::ai::agent_conversations_model::AgentConversationsModel;
use crate::ai::blocklist::agent_view::orchestration_pill_bar_model::OrchestrationPillBarModel;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai_assistant::requests::REQUEST_LIMIT_INFO_CACHE_KEY;
use crate::cloud_object::model::persistence::CloudModel;
use crate::env_vars::manager::EnvVarCollectionManager;
use crate::notebooks::manager::NotebookManager;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::sync_queue::SyncQueue;
use crate::server::telemetry::TelemetryEvent;
use crate::settings::{
    CloudPreferencesSettings, PrivacySettings, CRASH_REPORTING_ENABLED_DEFAULTS_KEY,
    TELEMETRY_ENABLED_DEFAULTS_KEY,
};
use crate::terminal::shared_session::manager::Manager as SharedSessionManager;
use crate::workflows::manager::WorkflowManager;
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::{persistence, send_telemetry_sync_from_app_ctx, GlobalResourceHandlesProvider};

pub fn init(app: &mut AppContext) {
    auth_view_modal::init(app);
    auth_view_body::init(app);
    auth_override_warning_body::init(app);
    login_slide::init(app);
    paste_auth_token_modal::init(app);
}

// Log out the user, clears workspace state, stops running processes, and deletes database.
pub fn log_out(app: &mut AppContext) {
    send_telemetry_sync_from_app_ctx!(TelemetryEvent::LogOut, app);

    CodebaseIndexManager::handle(app).update(app, |index_manager, ctx| {
        index_manager.reset_codebase_indexing(ctx);
    });

    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(app).get();

    // As part of Logout v0, we remove sqlite3 so sessions and cloud objects don't persist between accounts.
    // TODO: Implement per-user scoping of sqlite3.
    persistence::remove(&global_resource_handles.model_event_sender);

    AuthManager::handle(app).update(app, |auth_manager, ctx| {
        auth_manager.log_out(ctx);
    });
    AIExecutionProfilesModel::handle(app).update(app, |ai_execution_profiles_model, _| {
        ai_execution_profiles_model.reset();
    });
    BlocklistAIHistoryModel::handle(app).update(app, |history_model, _| {
        history_model.reset();
    });
    OrchestrationPillBarModel::handle(app).update(app, |pill_bar_model, _| {
        pill_bar_model.reset();
    });
    AgentConversationsModel::handle(app).update(app, |agent_conversations_model, _| {
        agent_conversations_model.reset();
    });
    CloudModel::handle(app).update(app, |cloud_model, _| {
        cloud_model.reset();
    });
    // Clear the sync queue so that we don't try to sync the old user's objects to the new user.
    SyncQueue::handle(app).update(app, |sync_queue, _| {
        sync_queue.clear();
    });

    // Stop the cloud object and workspace metadata polling loops that were started on login.
    UpdateManager::handle(app).update(app, |manager, _| {
        manager.stop_polling_for_updated_objects();
    });
    TeamUpdateManager::handle(app).update(app, |manager, _| {
        manager.stop_polling_for_workspace_metadata_updates();
    });
    remove_cloud_persisted_settings(app);
    NotebookManager::handle(app).update(app, |manager, _| manager.reset());
    EnvVarCollectionManager::handle(app).update(app, |manager, _| manager.reset());
    WorkflowManager::handle(app).update(app, |manager, _| manager.reset());

    // Stop and leave all shared sessions
    SharedSessionManager::handle(app).update(app, |manager, ctx| {
        manager.stop_all_shared_sessions(ctx);
        manager.clear_joined();
    });

    // Dispatch action on root view of every open window so the state can be updated
    // correctly.
    let window_ids = app.window_ids().collect_vec();
    for window_id in window_ids {
        if let Some(root_view_id) = app.root_view_id(window_id) {
            app.dispatch_action(
                window_id,
                &[root_view_id],
                "root_view:log_out",
                &(),
                log::Level::Info,
            );
        }
    }

    #[cfg(target_family = "wasm")]
    crate::platform::wasm::emit_event(crate::platform::wasm::WarpEvent::LoggedOut);
}

// Remove the cloud persisted settings from user defaults.
// When a user signs out, we remove cloud persisted settings of their account.
// This is so they do not experience the old settings when they log in with a different account.
// Partial deletion of user defaults is a stopgap for Logout v0. The correct solution is:
fn remove_cloud_persisted_settings(app: &mut AppContext) {
    let is_settings_sync_enabled = *CloudPreferencesSettings::as_ref(app).settings_sync_enabled;
    if is_settings_sync_enabled {
        SettingsManager::handle(app).update(app, |settings_manager, ctx| {
            let errors = settings_manager.clear_cloud_settings_local_state(ctx);
            for e in errors {
                log::error!("Failed to remove cloud synced setting from user defaults: {e:?}");
            }
        });
    }

    if let Err(e) = app
        .private_user_preferences()
        .remove_value(TELEMETRY_ENABLED_DEFAULTS_KEY)
    {
        log::error!("Failed to remove Telemetry Enabled Defaults Key from user defaults: {e:?}");
    }

    if let Err(e) = app
        .private_user_preferences()
        .remove_value(CRASH_REPORTING_ENABLED_DEFAULTS_KEY)
    {
        log::error!(
            "Failed to remove Crash Reporting Enabled Defaults Key from user defaults: {e:?}"
        );
    }

    if let Err(e) = app
        .private_user_preferences()
        .remove_value(REQUEST_LIMIT_INFO_CACHE_KEY)
    {
        log::error!("Failed to remove Request Limit Defaults Key from user defaults: {e:?}");
    }

    // Reset the Privacy Settings in the login screen to default values.
    PrivacySettings::handle(app).update(app, |privacy_settings, _| {
        privacy_settings.refresh_to_default();
    });
}
