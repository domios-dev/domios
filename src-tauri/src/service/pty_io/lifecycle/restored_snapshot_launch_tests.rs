use super::{
    restore_materialization_core, restored_snapshot_launch_command, restored_snapshot_launch_input,
    RestoredLaunch, RestoredSnapshotLaunchInput,
};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    AgentRole, PendingDeliveryChannel, PendingDeliveryRecord, RestorableLaunchParameters,
};
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::Arc;

fn restored_worker(
    historical_task: Option<&str>,
) -> (MuxState, String, String, String, Option<String>) {
    let (mut state, parent_id) = MultiplexerState::new();
    let workspace_id = state.find_workspace_for_tab(&parent_id).unwrap();
    state.set_orchestrator_role(&parent_id).unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&parent_id)
        .unwrap()
        .team_id;
    let tab_id = state.create_tab(&workspace_id).unwrap();
    state
        .set_tab_role(&tab_id, AgentRole::Worker, Some(&parent_id))
        .unwrap();
    let historical_task = historical_task.map(str::to_string);
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("fixture-session".into());
    tab.agent_session_model = Some("fixture-model".into());
    tab.restorable_launch = RestorableLaunchParameters {
        captured: true,
        identity_instructions: Some("opaque-worker-identity-instructions".into()),
        permission_bypass_requested: true,
        ..Default::default()
    };
    tab.cwd = Some("fixture-cwd".into());
    tab.role_profile = Some("fixture-profile".into());
    tab.restored_from_snapshot = true;
    if historical_task.is_some() {
        tab.pending_delivery = Some(PendingDeliveryRecord::new(
            "historical-digest".into(),
            PendingDeliveryChannel::InteractiveInput,
        ));
    }
    (
        Arc::new(TimedRwLock::new("restored-snapshot-no-replay", state)),
        tab_id,
        parent_id,
        team_id,
        historical_task,
    )
}

#[test]
fn restored_materialization_preserves_worker_identity_without_assignment_replay() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let permission_bypass_authorization =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true);
    for historical_task in [Some("historical assigned task"), None] {
        let (mux, tab_id, parent_id, team_id, historical_task) = restored_worker(historical_task);
        let was_assigned = historical_task.is_some();
        let observed = parking_lot::Mutex::new(None::<RestoredSnapshotLaunchInput>);

        let launch = restore_materialization_core(
            &mux,
            &tab_id,
            "claude",
            Some("fixture-session"),
            Some("fixture-cwd"),
            true,
            false,
            || Ok(historical_task.clone()),
            |_, _| panic!("restored fixture must not fail"),
            |launch, restored_task| {
                *observed.lock() = Some(restored_snapshot_launch_input(
                    &registry,
                    launch,
                    restored_task,
                    permission_bypass_authorization.clone(),
                ));
            },
        )
        .unwrap()
        .unwrap();
        let input = observed.into_inner().unwrap();
        let tab = mux.read().get_tab(&tab_id).unwrap().clone();

        assert_eq!(launch.provider_id, "claude");
        assert_eq!(input.params.agent.as_deref(), Some("claude"));
        assert!(input.params.permission_bypass_requested("claude"));
        let command = crate::service::orchestrator::build_agent_command(
            "claude",
            &input.params,
            input.launch_task.as_deref(),
            None,
            &registry,
        );
        assert!(command.contains("--dangerously-skip-permissions"));
        assert_eq!(
            input.params.resume_session_id.as_deref(),
            Some("fixture-session")
        );
        assert_eq!(input.params.cwd.as_deref(), Some("fixture-cwd"));
        assert_eq!(input.params.model.as_deref(), Some("fixture-model"));
        assert_eq!(input.params.role.as_deref(), Some("Worker"));
        assert_eq!(
            input.params.role_profile.as_deref(),
            Some("fixture-profile")
        );
        assert_eq!(input.params.task, None);
        assert_eq!(
            input.params.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
        let identity = input.launch_task.as_deref().unwrap();
        assert_eq!(identity, "opaque-worker-identity-instructions");
        assert!(!identity.contains("historical assigned task"));
        assert!(input.fallback_spec.is_none());
        assert!(matches!(
            input.origin,
            crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot
        ));
        assert_eq!(tab.parent_tab_id.as_deref(), Some(parent_id.as_str()));
        assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
        assert_eq!(tab.agent_session_id.as_deref(), Some("fixture-session"));
        assert_eq!(tab.agent_session_model.as_deref(), Some("fixture-model"));
        assert_eq!(tab.cwd.as_deref(), Some("fixture-cwd"));
        assert_eq!(tab.role, Some(AgentRole::Worker));
        assert_eq!(tab.role_profile.as_deref(), Some("fixture-profile"));
        assert_eq!(tab.pending_delivery.is_some(), was_assigned);
    }
}

#[test]
fn restored_team_member_resume_carries_bypass_only_when_granted() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let flags = registry
        .with_provider("claude", |provider| provider.auto_approve_flags())
        .unwrap();
    assert!(!flags.is_empty());
    for granted in [true, false] {
        let (mux, tab_id, _, team_id, _) = restored_worker(None);
        let authorization = crate::service::permission_policy::authorize_permission_bypass(
            &registry, "claude", granted,
        );
        let observed = parking_lot::Mutex::new(None::<RestoredSnapshotLaunchInput>);
        restore_materialization_core(
            &mux,
            &tab_id,
            "claude",
            Some("fixture-session"),
            Some("fixture-cwd"),
            true,
            false,
            || Ok(None),
            |_, _| panic!("restored fixture must not fail"),
            |launch, restored_task| {
                *observed.lock() = Some(restored_snapshot_launch_input(
                    &registry,
                    launch,
                    restored_task,
                    authorization.clone(),
                ));
            },
        )
        .unwrap()
        .unwrap();
        let input = observed.into_inner().unwrap();
        let command = crate::service::orchestrator::build_agent_command(
            "claude",
            &input.params,
            input.launch_task.as_deref(),
            None,
            &registry,
        );

        assert_eq!(
            mux.read().get_tab(&tab_id).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
        assert!(command.contains("fixture-session"));
        assert_eq!(input.params.permission_bypass_requested("claude"), granted);
        for flag in &flags {
            assert_eq!(command.contains(flag.as_str()), granted, "{command}");
        }
    }
}

#[test]
fn legacy_snapshot_restore_uses_current_policy_bypass() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let authorization = crate::service::permission_policy::authorize_permission_bypass(
        &registry, "claude", true,
    );
    let input = restored_snapshot_launch_input(
        &registry,
        RestoredLaunch {
            provider_id: "claude".into(),
            session_id: Some("legacy-session".into()),
            cwd: Some("legacy-cwd".into()),
            model: None,
            launch_parameters: RestorableLaunchParameters::default(),
            role: None,
            role_profile: None,
        },
        None,
        authorization,
    );

    assert!(input.params.permission_bypass_requested("claude"));
    assert!(input.launch_parameters.permission_bypass_requested);
}

fn codex_effort_registry() -> crate::agent::registry::AgentRegistry {
    let registry = crate::agent::registry::AgentRegistry::new();
    let effort = crate::agent::provider::ReasoningEffortInfo {
        id: "xhigh".into(),
        display_name: "xhigh".into(),
        description: None,
    };
    let model = crate::agent::provider::ModelInfo {
        id: "gpt-6-luna".into(),
        display_name: "gpt-6-luna".into(),
        reasoning_efforts: Some(vec![effort]),
        ..Default::default()
    };
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_system_prompt_flag(
            "codex-fixture",
            "--instructions",
        )
        .with_models(vec![model]),
    ));
    registry
}

fn parsed_flag_value(command: &str, flag: &str) -> Option<String> {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].to_string())
}

fn launch_parameters() -> RestorableLaunchParameters {
    RestorableLaunchParameters {
        captured: true,
        requested_model: Some("gpt-6-luna".into()),
        model: Some("gpt-6-luna".into()),
        requested_reasoning_effort: Some("xhigh".into()),
        resolved_reasoning_effort: Some("xhigh".into()),
        args: Some(vec!["--approval-mode".into(), "restricted".into()]),
        agent_name: Some("restore-agent".into()),
        identity_instructions: Some("opaque-instruction-fragment".into()),
        permission_bypass_requested: false,
    }
}

fn codex_restore_launch() -> RestoredLaunch {
    RestoredLaunch {
        provider_id: "codex-fixture".into(),
        session_id: None,
        cwd: Some("fixture-cwd".into()),
        model: Some("gpt-6-luna".into()),
        launch_parameters: launch_parameters(),
        role: Some("Worker".into()),
        role_profile: Some("executor".into()),
    }
}

#[test]
fn restored_codex_launch_keeps_model_and_reasoning_effort() {
    let registry = codex_effort_registry();
    let input = restored_snapshot_launch_input(&registry, codex_restore_launch(), None, None);
    let command =
        restored_snapshot_launch_command(&registry, "tab-restored-codex", "codex-fixture", &input);

    assert_eq!(
        parsed_flag_value(&command, "--model").as_deref(),
        Some("gpt-6-luna")
    );
    assert_eq!(
        parsed_flag_value(&command, "--effort").as_deref(),
        Some("xhigh")
    );
}

#[test]
fn restore_keeps_every_launch_shaping_parameter_named_in_the_proposal() {
    let registry = codex_effort_registry();
    let input = restored_snapshot_launch_input(&registry, codex_restore_launch(), None, None);
    let command = restored_snapshot_launch_command(
        &registry,
        "tab-restored-parameters",
        "codex-fixture",
        &input,
    );

    assert_eq!(input.params.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(input.params.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(
        input
            .params
            .args
            .as_deref()
            .map(|args| args.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["--approval-mode", "restricted"])
    );
    assert_eq!(input.params.agent_name.as_deref(), Some("restore-agent"));
    assert_eq!(
        input.launch_task.as_deref(),
        Some("opaque-instruction-fragment")
    );
    assert!(!input.params.permission_bypass_requested("codex-fixture"));
    assert_eq!(
        parsed_flag_value(&command, "--model").as_deref(),
        Some("gpt-6-luna")
    );
    assert_eq!(
        parsed_flag_value(&command, "--effort").as_deref(),
        Some("xhigh")
    );
    assert_eq!(
        parsed_flag_value(&command, "--approval-mode").as_deref(),
        Some("restricted")
    );
}

#[test]
fn inspect_reports_the_restored_launch_parameters() {
    let registry = codex_effort_registry();
    let (mut state, tab_id) = MultiplexerState::new();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_model = Some("stale-model".into());
        tab.reasoning_effort_identity.actual = Some("stale-effort".into());
        tab.restorable_launch = launch_parameters();
    }
    let input = restored_snapshot_launch_input(&registry, codex_restore_launch(), None, None);
    let mux = Arc::new(TimedRwLock::new("restored-launch-inspect", state));
    crate::service::restored_launch::persist_restored_launch_identity(
        &mux,
        &tab_id,
        "codex-fixture",
        &input.params,
        &input.launch_parameters,
    );
    let mux = mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();

    assert_eq!(tab.agent_session_model, None);
    assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some("gpt-6-luna")
    );
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("xhigh")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("xhigh")
    );
    assert_eq!(tab.reasoning_effort_identity.actual, None);
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("provider-default-unverified")
    );
}

#[test]
fn restored_snapshot_fresh_launch_carries_the_tabs_marker_and_resume_carries_none() {
    use crate::service::launch_session_detection::registered_launch_marker;

    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "snapshot-marked",
    )));
    let restored = |session_id: Option<&str>| RestoredLaunch {
        provider_id: "snapshot-marked".into(),
        session_id: session_id.map(str::to_string),
        cwd: Some("fixture-cwd".into()),
        model: None,
        launch_parameters: RestorableLaunchParameters::default(),
        role: None,
        role_profile: None,
    };

    let fresh = restored_snapshot_launch_input(&registry, restored(None), None, None);
    let command = restored_snapshot_launch_command(
        &registry,
        "tab-snapshot-fresh",
        "snapshot-marked",
        &fresh,
    );
    let marker = registered_launch_marker("tab-snapshot-fresh")
        .expect("restored fresh launch registers its launch marker");
    assert!(!marker.is_empty());
    assert!(
        command.ends_with(&format!(" --launch-marker {marker}")),
        "{command}"
    );

    let resume =
        restored_snapshot_launch_input(&registry, restored(Some("session-snapshot")), None, None);
    let resumed = restored_snapshot_launch_command(
        &registry,
        "tab-snapshot-resume",
        "snapshot-marked",
        &resume,
    );
    assert!(!resumed.contains("--launch-marker"), "{resumed}");
    assert_eq!(registered_launch_marker("tab-snapshot-resume"), None);
}
