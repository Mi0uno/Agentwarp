use chrono::Utc;
use std::path::PathBuf;
use warp_graphql::scalars::time::ServerTimestamp;
use warpui::{App, SingletonEntity};

use super::*;
use crate::ai::request_usage_model::{RequestLimitInfo, RequestLimitRefreshDuration};
use crate::auth::AuthStateProvider;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn create_test_request_limit_info(
    limit: usize,
    used: usize,
    next_refresh: DateTime<Utc>,
    is_unlimited: bool,
    refresh_duration: RequestLimitRefreshDuration,
) -> RequestLimitInfo {
    RequestLimitInfo {
        limit,
        num_requests_used_since_refresh: used,
        next_refresh_time: ServerTimestamp::new(next_refresh),
        is_unlimited,
        request_limit_refresh_duration: refresh_duration,
        is_unlimited_voice: false,
        voice_request_limit: 0,
        voice_requests_used_since_last_refresh: 0,
        is_unlimited_codebase_indices: false,
        max_codebase_indices: 0,
        max_files_per_repo: 5000,
        embedding_generation_batch_size: 100,
    }
}

fn add_ai_enablement_dependencies_for_test(app: &mut App) {
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(UserWorkspaces::default_mock);
}

fn enable_cli_agent_api_takeover_for_test(app: &mut App) {
    AISettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(settings.cli_agent_api_takeover_enabled.set_value(true, ctx));
    });
}

#[test]
fn warp_agent_defaults_to_disabled() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(!settings.is_any_ai_enabled(ctx));
        });
    });
}

// FocusedTerminalInfo Tests

#[test]
fn test_update_both_values_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // Update both values to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_additional_value_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Now update to (true, true) - only changing restored blocks
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_no_change() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with same values (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state remains the same
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify no event was emitted
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 0);
    });
}

#[test]
fn test_update_only_remote_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (false, true) - only remote blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(false, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(!model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_only_restored_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (true, false) - only restored blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

// ToolbarCommandMap Tests

#[test]
fn test_toolbar_command_map_deserialize_from_map() {
    let json = serde_json::json!({
        "^claude": "Claude",
        "^gemini": "Gemini",
        "^codex": ""
    });
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^gemini"], "Gemini");
    assert_eq!(map.0["^codex"], "");
}

#[test]
fn test_toolbar_command_map_deserialize_from_legacy_vec() {
    let json = serde_json::json!(["^claude", "^gemini", "^custom"]);
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    // Legacy vec format should assign empty agent values.
    for (_, agent) in map.0.iter() {
        assert_eq!(agent, "");
    }
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^claude", "^gemini", "^custom"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_map_format() {
    use settings_value::SettingsValue;

    let value = serde_json::json!({
        "^claude": "Claude",
        "^amp": "Amp"
    });
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 2);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^amp"], "Amp");
}

#[test]
fn test_toolbar_command_map_from_file_value_legacy_array() {
    use settings_value::SettingsValue;

    // Patterns are intentionally non-alphabetical to verify insertion order is preserved.
    let value = serde_json::json!(["^zebra", "^alpha", "^middle"]);
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^zebra"], "");
    assert_eq!(map.0["^alpha"], "");
    assert_eq!(map.0["^middle"], "");
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^zebra", "^alpha", "^middle"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_invalid() {
    use settings_value::SettingsValue;

    let value = serde_json::json!(42);
    assert!(ToolbarCommandMap::from_file_value(&value).is_none());
}

#[test]
fn test_toolbar_command_map_roundtrip() {
    use settings_value::SettingsValue;

    let mut inner = IndexMap::new();
    inner.insert("^claude".to_string(), "Claude".to_string());
    inner.insert("^custom".to_string(), String::new());
    let original = ToolbarCommandMap::new(inner);

    let file_value = original.to_file_value();
    let restored = ToolbarCommandMap::from_file_value(&file_value).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_cli_agent_builtin_prompt_applies_append_mode() {
    let prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Append,
        prompt: "Prefer concise answers.".to_string(),
    };

    assert_eq!(
        prompt.apply_to_prompt("Explain the diff.".to_string()),
        "Explain the diff.\n\nAdditional built-in instructions:\nPrefer concise answers."
    );
}

#[test]
fn test_cli_agent_builtin_prompt_applies_replace_mode() {
    let prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Replace,
        prompt: "You are a careful code reviewer.".to_string(),
    };

    assert_eq!(
        prompt.apply_to_prompt("Review this patch.".to_string()),
        "Built-in system prompt:\nYou are a careful code reviewer.\n\nUser request:\nReview this patch."
    );
}

#[test]
fn test_cli_agent_builtin_prompt_builds_claude_native_launch_suffix() {
    let append_prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Append,
        prompt: "Prefer concise answers.".to_string(),
    };
    assert_eq!(
        append_prompt
            .native_launch_suffix(CLIAgent::Claude)
            .as_deref(),
        Some("--append-system-prompt 'Prefer concise answers.'")
    );

    let replace_prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Replace,
        prompt: "You are a reviewer.".to_string(),
    };
    assert_eq!(
        replace_prompt
            .native_launch_suffix(CLIAgent::Claude)
            .as_deref(),
        Some("--system-prompt 'You are a reviewer.'")
    );
}

#[test]
fn test_cli_agent_builtin_prompt_builds_codex_native_launch_suffix() {
    let prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Append,
        prompt: "Prefer concise answers.".to_string(),
    };

    assert_eq!(
        prompt.native_launch_suffix(CLIAgent::Codex).as_deref(),
        Some("-c 'developer_instructions=\"Prefer concise answers.\"'")
    );
}

#[test]
fn test_cli_agent_builtin_prompt_has_no_opencode_native_launch_suffix() {
    let prompt = CLIAgentBuiltinPrompt {
        mode: CLIAgentBuiltinPromptMode::Append,
        prompt: "Prefer concise answers.".to_string(),
    };

    assert_eq!(prompt.native_launch_suffix(CLIAgent::OpenCode), None);
}

#[test]
fn test_cli_agent_builtin_prompt_supported_agents() {
    assert!(AISettings::supports_cli_agent_builtin_prompt(
        CLIAgent::Claude
    ));
    assert!(AISettings::supports_cli_agent_builtin_prompt(
        CLIAgent::Codex
    ));
    assert!(AISettings::supports_cli_agent_builtin_prompt(
        CLIAgent::OpenCode
    ));
    assert!(!AISettings::supports_cli_agent_builtin_prompt(
        CLIAgent::Gemini
    ));
}

#[test]
fn test_cli_agent_api_profile_store_prefers_environment_specific_profile() {
    let mut store = CLIAgentApiProfilesStore::default();
    let all_profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned(),
        "Shared".to_owned(),
        "https://shared.example.com".to_owned(),
        "shared-key".to_owned(),
        "gpt-shared".to_owned(),
    );
    let remote_profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        "ssh:devbox".to_owned(),
        "Remote".to_owned(),
        "https://remote.example.com".to_owned(),
        "remote-key".to_owned(),
        "gpt-remote".to_owned(),
    );

    store.upsert_profile(all_profile.clone(), true);
    store.upsert_profile(remote_profile.clone(), true);

    assert_eq!(
        store
            .active_profile(CLIAgent::Codex, "ssh:devbox")
            .map(|profile| profile.id.as_str()),
        Some(remote_profile.id.as_str())
    );
    assert_eq!(
        store
            .active_profile(CLIAgent::Codex, "ssh:other")
            .map(|profile| profile.id.as_str()),
        Some(all_profile.id.as_str())
    );
}

#[test]
fn test_cli_agent_api_profile_store_ignores_disabled_profiles() {
    let mut store = CLIAgentApiProfilesStore::default();
    let mut disabled_profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
        "Disabled".to_owned(),
        "https://disabled.example.com".to_owned(),
        "disabled-key".to_owned(),
        "gpt-disabled".to_owned(),
    );
    disabled_profile.enabled = false;

    store.upsert_profile(disabled_profile, true);

    assert!(store
        .active_profile(CLIAgent::Codex, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
        .is_none());
    assert!(store
        .fallback_profiles(CLIAgent::Codex, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
        .is_empty());
}

#[test]
fn test_cli_agent_api_profile_store_disabling_profile_clears_active_profile() {
    let mut store = CLIAgentApiProfilesStore::default();
    let profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
        "Primary".to_owned(),
        "https://primary.example.com".to_owned(),
        "primary-key".to_owned(),
        "gpt-primary".to_owned(),
    );
    let profile_id = profile.id.clone();

    store.upsert_profile(profile, true);
    store.set_profile_enabled(&profile_id, false);

    assert!(store
        .active_profile(CLIAgent::Codex, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
        .is_none());
    assert!(store
        .fallback_profiles(CLIAgent::Codex, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
        .is_empty());
    assert!(store.active_profile_ids.is_empty());
}

#[test]
fn test_cli_agent_api_profile_store_upsert_scope_change_clears_stale_active_profile() {
    let mut store = CLIAgentApiProfilesStore::default();
    let profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
        "Primary".to_owned(),
        "https://primary.example.com".to_owned(),
        "primary-key".to_owned(),
        "gpt-primary".to_owned(),
    );
    let mut moved_profile = profile.clone();
    moved_profile.environment_id = "ssh:devbox".to_owned();

    store.upsert_profile(profile, true);
    store.upsert_profile(moved_profile, false);

    assert!(store
        .active_profile(CLIAgent::Codex, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
        .is_none());
    assert!(store.active_profile_ids.is_empty());
}

#[test]
fn test_cli_agent_api_profile_store_imports_profile_list_json() {
    let profile = CLIAgentApiProfile::new(
        CLIAgent::Claude,
        CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
        "Claude relay".to_owned(),
        "https://claude.example.com".to_owned(),
        "claude-key".to_owned(),
        "sonnet".to_owned(),
    );
    let json = serde_json::to_string(&vec![profile.clone()]).unwrap();

    let imported_store = CLIAgentApiProfilesStore::from_import_json(&json).unwrap();

    assert_eq!(imported_store.profiles.len(), 1);
    assert_eq!(imported_store.profiles[0].id, profile.id);
    assert_eq!(imported_store.profiles[0].agent(), CLIAgent::Claude);
}

#[test]
fn test_cli_agent_api_profile_store_imports_store_json_with_active_profile() {
    let mut store = CLIAgentApiProfilesStore::default();
    let profile = CLIAgentApiProfile::new(
        CLIAgent::Gemini,
        CLI_AGENT_API_ALL_ENVIRONMENTS_ID.to_owned(),
        "Gemini relay".to_owned(),
        "https://gemini.example.com".to_owned(),
        "gemini-key".to_owned(),
        "gemini-pro".to_owned(),
    );
    let profile_id = profile.id.clone();

    store.upsert_profile(profile, true);
    let json = serde_json::to_string(&store).unwrap();
    let imported_store = CLIAgentApiProfilesStore::from_import_json(&json).unwrap();

    assert_eq!(
        imported_store
            .active_profile(CLIAgent::Gemini, CLI_AGENT_API_LOCAL_ENVIRONMENT_ID)
            .map(|profile| profile.id.as_str()),
        Some(profile_id.as_str())
    );
}

#[test]
fn test_cli_agent_api_profile_store_merge_imported_profiles() {
    let existing_profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
        "Existing".to_owned(),
        "https://existing.example.com".to_owned(),
        "existing-key".to_owned(),
        "gpt-existing".to_owned(),
    );
    let imported_profile = CLIAgentApiProfile::new(
        CLIAgent::Codex,
        "ssh:devbox".to_owned(),
        "Imported".to_owned(),
        "https://imported.example.com".to_owned(),
        "imported-key".to_owned(),
        "gpt-imported".to_owned(),
    );
    let imported_profile_id = imported_profile.id.clone();
    let mut store = CLIAgentApiProfilesStore::default();
    let mut imported_store = CLIAgentApiProfilesStore::default();

    store.upsert_profile(existing_profile, true);
    imported_store.upsert_profile(imported_profile, true);
    let imported_count = store.merge_store(imported_store);

    assert_eq!(imported_count, 1);
    assert_eq!(store.profiles.len(), 2);
    assert_eq!(
        store
            .active_profile(CLIAgent::Codex, "ssh:devbox")
            .map(|profile| profile.id.as_str()),
        Some(imported_profile_id.as_str())
    );
}

#[test]
fn test_cli_agent_api_profile_health_display_labels() {
    assert_eq!(
        CLIAgentApiProfileHealth::default().display_label(),
        "health unchecked"
    );
    assert_eq!(
        CLIAgentApiProfileHealth::checking(123).display_label(),
        "health checking"
    );
    assert_eq!(
        CLIAgentApiProfileHealth::healthy(123, 45, 200).display_label(),
        "healthy 45ms HTTP 200"
    );
    assert_eq!(
        CLIAgentApiProfileHealth::failed(123, 45, 401, "Unauthorized").display_label(),
        "failed HTTP 401 Unauthorized"
    );
}

#[test]
fn test_cli_agent_api_usage_summary_display_label() {
    let summary = CLIAgentApiUsageSummary {
        log_path: PathBuf::from("/tmp/agent-api-usage.ndjson"),
        event_count: 3,
        successful_events: 2,
        failed_events: 1,
        retry_events: 1,
        total_latency_ms: 90,
        total_request_bytes: 120,
        total_response_bytes: 300,
        total_prompt_tokens: 40,
        total_completion_tokens: 20,
        total_tokens: 60,
        total_estimated_cost_usd: 0.0123,
        last_profile_name: "Primary".to_owned(),
        last_status: 200,
        last_error: String::new(),
    };

    assert_eq!(
        summary.display_label(),
        "3 events / 2 success / 1 failed / 1 failover / avg 30ms / in 120B / out 300B / last HTTP 200 via Primary / 60 tokens (40 in / 20 out) / est $0.0123"
    );
}

#[test]
fn test_cli_agent_api_usage_summary_reads_token_usage() {
    let contents = r#"{"profile_name":"Primary","status":200,"success":true,"final_attempt":true,"latency_ms":10,"prompt_tokens":7,"completion_tokens":3,"estimated_cost_usd":0.0001}"#;

    let summary =
        cli_agent_api_usage_summary_from_contents(PathBuf::from("/tmp/usage.ndjson"), contents);

    assert_eq!(summary.total_prompt_tokens, 7);
    assert_eq!(summary.total_completion_tokens, 3);
    assert_eq!(summary.total_tokens, 10);
    assert_eq!(summary.total_estimated_cost_usd, 0.0001);
    assert_eq!(
        summary.display_label(),
        "1 events / 1 success / 0 failed / 0 failover / avg 10ms / in 0B / out 0B / last HTTP 200 via Primary / 10 tokens (7 in / 3 out) / est $0.0001"
    );
}

#[test]
fn test_cli_agent_api_environment_vars_empty_when_takeover_disabled() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_cli_agent_api_profile(
                CLIAgentApiProfile::new(
                    CLIAgent::Claude,
                    CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
                    "Claude relay".to_owned(),
                    "https://claude.example.com/".to_owned(),
                    "claude-key".to_owned(),
                    "claude-sonnet".to_owned(),
                ),
                true,
                ctx,
            );
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(!settings.is_cli_agent_api_takeover_enabled());
            assert!(settings
                .cli_agent_api_environment_vars(
                    CLIAgent::Claude,
                    CLI_AGENT_API_LOCAL_ENVIRONMENT_ID
                )
                .is_empty());
        });
    });
}

#[test]
fn test_cli_agent_api_environment_vars_use_agent_native_names() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_cli_agent_api_takeover_for_test(&mut app);

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_cli_agent_api_profile(
                CLIAgentApiProfile::new(
                    CLIAgent::Claude,
                    CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
                    "Claude relay".to_owned(),
                    "https://claude.example.com/".to_owned(),
                    "claude-key".to_owned(),
                    "claude-sonnet".to_owned(),
                ),
                true,
                ctx,
            );
            settings.add_cli_agent_api_profile(
                CLIAgentApiProfile::new(
                    CLIAgent::Gemini,
                    CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
                    "Gemini relay".to_owned(),
                    "https://gemini.example.com".to_owned(),
                    "gemini-key".to_owned(),
                    "gemini-pro".to_owned(),
                ),
                true,
                ctx,
            );
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let claude_env = settings.cli_agent_api_environment_vars(
                CLIAgent::Claude,
                CLI_AGENT_API_LOCAL_ENVIRONMENT_ID,
            );
            assert_eq!(
                claude_env.get("ANTHROPIC_API_KEY").map(String::as_str),
                Some("claude-key")
            );
            assert_eq!(
                claude_env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
                Some("claude-key")
            );
            assert_eq!(
                claude_env.get("ANTHROPIC_BASE_URL").map(String::as_str),
                Some("https://claude.example.com")
            );
            assert_eq!(
                claude_env.get("ANTHROPIC_MODEL").map(String::as_str),
                Some("claude-sonnet")
            );
            assert_eq!(
                claude_env
                    .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                    .map(String::as_str),
                Some("claude-sonnet")
            );
            assert_eq!(
                claude_env
                    .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                    .map(String::as_str),
                Some("claude-sonnet")
            );
            assert_eq!(
                claude_env
                    .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                    .map(String::as_str),
                Some("claude-sonnet")
            );

            let gemini_env = settings.cli_agent_api_environment_vars(
                CLIAgent::Gemini,
                CLI_AGENT_API_LOCAL_ENVIRONMENT_ID,
            );
            assert_eq!(
                gemini_env.get("GEMINI_API_KEY").map(String::as_str),
                Some("gemini-key")
            );
            assert_eq!(
                gemini_env.get("GOOGLE_GEMINI_BASE_URL").map(String::as_str),
                Some("https://gemini.example.com")
            );
        });
    });
}

#[test]
fn test_cli_agent_api_environment_vars_include_claude_model_mappings() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_cli_agent_api_takeover_for_test(&mut app);

        let mut profile = CLIAgentApiProfile::new(
            CLIAgent::Claude,
            CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
            "Claude relay".to_owned(),
            "https://claude.example.com".to_owned(),
            "claude-key".to_owned(),
            "fallback-model".to_owned(),
        );
        profile.model_mappings = vec![
            CLIAgentApiModelMapping {
                role: "Sonnet".to_owned(),
                display_name: "DeepSeek V4 Pro".to_owned(),
                model: "deepseek-v4-pro".to_owned(),
                supports_one_million_context: true,
                context_window_tokens: 0,
            },
            CLIAgentApiModelMapping {
                role: "Opus".to_owned(),
                display_name: "DeepSeek V4 Ultra".to_owned(),
                model: "deepseek-v4-ultra[1M]".to_owned(),
                supports_one_million_context: true,
                context_window_tokens: 0,
            },
            CLIAgentApiModelMapping {
                role: "Haiku".to_owned(),
                display_name: String::new(),
                model: "deepseek-v4-flash".to_owned(),
                supports_one_million_context: false,
                context_window_tokens: 0,
            },
        ];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_cli_agent_api_profile(profile, true, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let env = settings.cli_agent_api_environment_vars(
                CLIAgent::Claude,
                CLI_AGENT_API_LOCAL_ENVIRONMENT_ID,
            );
            assert_eq!(
                env.get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                    .map(String::as_str),
                Some("deepseek-v4-pro[1M]")
            );
            assert_eq!(
                env.get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
                    .map(String::as_str),
                Some("DeepSeek V4 Pro")
            );
            assert_eq!(
                env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").map(String::as_str),
                Some("deepseek-v4-ultra[1M]")
            );
            assert_eq!(
                env.get("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
                    .map(String::as_str),
                Some("DeepSeek V4 Ultra")
            );
            assert_eq!(
                env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").map(String::as_str),
                Some("deepseek-v4-flash")
            );
            assert!(!env.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"));
        });
    });
}

#[test]
fn test_cli_agent_api_environment_vars_include_failover_chain() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_cli_agent_api_takeover_for_test(&mut app);

        let mut primary = CLIAgentApiProfile::new(
            CLIAgent::Codex,
            CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
            "Primary".to_owned(),
            "https://primary.example.com".to_owned(),
            "primary-key".to_owned(),
            "gpt-primary".to_owned(),
        );
        primary.priority = 10;
        let mut fallback = CLIAgentApiProfile::new(
            CLIAgent::Codex,
            CLI_AGENT_API_LOCAL_ENVIRONMENT_ID.to_owned(),
            "Fallback".to_owned(),
            "https://fallback.example.com".to_owned(),
            "fallback-key".to_owned(),
            "gpt-fallback".to_owned(),
        );
        fallback.priority = 20;
        let primary_id = primary.id.clone();
        let fallback_id = fallback.id.clone();

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_cli_agent_api_profile(primary, true, ctx);
            settings.add_cli_agent_api_profile(fallback, false, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let env = settings.cli_agent_api_environment_vars(
                CLIAgent::Codex,
                CLI_AGENT_API_LOCAL_ENVIRONMENT_ID,
            );
            assert_eq!(
                env.get("AGENTWARP_AGENT_API_FAILOVER_ENABLED")
                    .map(String::as_str),
                Some("1")
            );
            assert_eq!(
                env.get("AGENTWARP_AGENT_API_PROFILE_COUNT")
                    .map(String::as_str),
                Some("2")
            );

            let fallback_profiles = serde_json::from_str::<Vec<CLIAgentApiProfile>>(
                env.get("AGENTWARP_AGENT_API_FALLBACKS").unwrap(),
            )
            .unwrap();
            assert_eq!(fallback_profiles[0].id, primary_id);
            assert_eq!(fallback_profiles[1].id, fallback_id);
            assert_eq!(fallback_profiles[1].api_key, "fallback-key");
        });
    });
}

#[test]
fn test_toolbar_command_map_matched_agent() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let mut map = IndexMap::new();
        map.insert("^claude".to_string(), "Claude".to_string());
        map.insert("^gemini".to_string(), "Gemini".to_string());
        map.insert("^custom-tool".to_string(), String::new());

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            report_if_error!(settings
                .cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx));
        });

        app.read(|ctx| {
            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "claude chat");
            assert_eq!(agent, Some(CLIAgent::Claude));

            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "gemini ask");
            assert_eq!(agent, Some(CLIAgent::Gemini));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "custom-tool --flag");
            assert_eq!(agent, Some(CLIAgent::Unknown));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "unmatched-command");
            assert_eq!(agent, None);
        });
    });
}

#[test]
fn orchestration_v2_enables_orchestration_when_ai_is_enabled() {
    let _orchestration_v2_flag = FeatureFlag::OrchestrationV2.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(settings.is_orchestration_enabled(ctx));
        });
    });
}

#[test]
fn orchestration_v2_disabled_disables_orchestration() {
    let _orchestration_v2_flag = FeatureFlag::OrchestrationV2.override_enabled(false);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(!settings.is_orchestration_enabled(ctx));
        });
    });
}
#[test]
fn test_should_display_quota_reset_banner_with_empty_history() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // With empty history, banner should not be displayed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_not_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded and banner not dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should be displayed when the previous cycle had quota exceeded and banner not dismissed
            assert!(settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded but banner was dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: true },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle had quota exceeded but banner was dismissed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_not_exceeded() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that did not have quota exceeded
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle did not have quota exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_only_one_cycle() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with only one cycle
        let now = Utc::now();
        let current_end_date = now + chrono::Duration::days(15);

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: true, // Even if quota is exceeded
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when there's only one cycle, even if quota is exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_update_quota_info_create_new_cycle_when_none_exists() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let next_refresh = now + chrono::Duration::days(30);

        // Create a request limit info with quota not exceeded
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            50,  // used
            next_refresh,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Ensure we start with empty history
            settings
                .ai_request_quota_info
                .set_value(
                    AIRequestQuotaInfo {
                        cycle_history: vec![],
                    },
                    ctx,
                )
                .unwrap();

            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify a new cycle was created
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 1);

            let cycle = &cycle_history[0];
            assert_eq!(cycle.end_date, next_refresh);
            assert!(!cycle.was_quota_exceeded);
            assert!(!cycle.banner_state.dismissed);
        });
    });
}

#[test]
fn test_update_quota_info_update_existing_cycle() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let cycle_end_date = now + chrono::Duration::days(30);

        // Set up an existing cycle
        let existing_cycle = CycleInfo {
            end_date: cycle_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(
                    AIRequestQuotaInfo {
                        cycle_history: vec![existing_cycle],
                    },
                    ctx,
                )
                .unwrap();
        });

        // Create a request limit info with updated usage
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            75,  // used (increased)
            cycle_end_date,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify the cycle was updated
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 1);

            let cycle = &cycle_history[0];
            assert_eq!(cycle.end_date, cycle_end_date);
            assert!(!cycle.was_quota_exceeded);
        });
    });
}

#[test]
fn test_update_quota_info_quota_exceeded() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let next_refresh = now + chrono::Duration::days(30);

        // Create a request limit info with quota exceeded
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            100, // used (equal to limit, should be marked as exceeded)
            next_refresh,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify quota exceeded is set correctly
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            let cycle = &cycle_history[0];
            assert!(cycle.was_quota_exceeded);
        });

        // Test with unlimited requests (should never be exceeded)
        let unlimited_request_limit_info = create_test_request_limit_info(
            100, // limit
            200, // used (exceeds limit)
            next_refresh,
            true, // unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&unlimited_request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify quota exceeded is not set for unlimited plan
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            let cycle = &cycle_history[0];
            assert!(!cycle.was_quota_exceeded);
        });
    });
}

#[test]
fn test_mark_quota_banner_as_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();

        // Create test cycles: two expired cycles and one future cycle
        let expired_cycle_1 = CycleInfo {
            end_date: now - chrono::Duration::days(30), // 30 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let expired_cycle_2 = CycleInfo {
            end_date: now - chrono::Duration::days(15), // 15 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let future_cycle = CycleInfo {
            end_date: now + chrono::Duration::days(15), // 15 days in future
            was_quota_exceeded: false,
            banner_state: BannerState { dismissed: false },
        };

        let cycle_history = vec![expired_cycle_1, expired_cycle_2, future_cycle];

        // Set up initial state
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        // Mark expired cycles as dismissed
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.mark_quota_banner_as_dismissed(ctx);
        });

        // Verify the results
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 3);

            // First cycle (oldest expired) should be dismissed
            assert!(cycle_history[0].banner_state.dismissed);
            // Second cycle (more recent expired) should be dismissed
            assert!(cycle_history[1].banner_state.dismissed);
            // Future cycle should not be dismissed
            assert!(!cycle_history[2].banner_state.dismissed);
        });
    });
}
