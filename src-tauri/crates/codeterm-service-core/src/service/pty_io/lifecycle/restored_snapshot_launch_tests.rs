use super::{
    claim_restored_launch, restore_materialization_core, restored_snapshot_launch_command,
    restored_snapshot_launch_input, RestoredLaunch, RestoredSnapshotLaunchInput,
};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    AgentRole, PendingDeliveryChannel, PendingDeliveryRecord, RestorableLaunchParameters,
};
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::Arc;

#[test]
fn restored_structured_prompt_server_is_registered_by_the_shared_launch_builder() {
    let (state, tab_id) = MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(state);
    let launches = Arc::new(parking_lot::Mutex::new(Vec::new()));
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("structured-restore-fixture")
            .with_structured_prompt_launch_log(launches.clone()),
    ));
    let cwd = fixture.temp.path().to_string_lossy().into_owned();
    let launch = RestoredLaunch {
        provider_id: "structured-restore-fixture".into(),
        session_id: Some("owned-restored-session".into()),
        cwd: Some(cwd.clone()),
        model: None,
        launch_parameters: RestorableLaunchParameters::default(),
        role: None,
        role_profile: None,
    };
    let input = restored_snapshot_launch_input(&fixture.service.agent_registry, launch, None, None);
    assert!(input.params.structured_prompt_server.is_none());
    assert!(crate::service::structured_endpoint::sse(&tab_id).is_none());

    let command = super::restored_snapshot_launch_command_for_service(
        &fixture.service,
        &tab_id,
        "structured-restore-fixture",
        &input,
    );

    let endpoint = crate::service::structured_endpoint::sse(&tab_id);
    crate::service::structured_prompt::clear_tab(&tab_id);
    assert!(!command.is_empty());
    let launches = launches.lock();
    assert_eq!(launches.len(), 1);
    assert_eq!(
        launches[0].session_id.as_deref(),
        Some("owned-restored-session")
    );
    let server = launches[0].structured_prompt_server.as_ref().unwrap();
    assert_eq!(server.hostname, "127.0.0.1");
    assert_ne!(server.port, 0);
    assert_eq!(
        endpoint,
        Some((format!("http://127.0.0.1:{}", server.port), cwd))
    );
    assert_eq!(
        crate::service::launch_session_detection::registered_launch_marker(&tab_id).as_deref(),
        Some("owned-restored-session")
    );
    crate::service::launch_session_detection::clear_launch_marker(&tab_id);
}

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
        context_files: None,
        context_skills: None,
        worker_restart: None,
        captured: true,
        identity_instructions: Some("opaque-worker-identity-instructions".into()),
        permission_bypass_requested: true,
        ..Default::default()
    };
    tab.cwd = Some("fixture-cwd".into());
    tab.role_profile = Some("fixture-profile".into());
    tab.restored_from_snapshot = true;
    if historical_task.is_some() {
        let mut pending = PendingDeliveryRecord::new(
            "historical-digest".into(),
            PendingDeliveryChannel::InteractiveInput,
        );
        pending
            .begin_attempt(crate::multiplexer::types::DeliveryAttemptProvenance {
                attempt_id: "historical-attempt".into(),
                started_at_ms: 1,
            })
            .unwrap();
        pending.mark_unconfirmed();
        tab.pending_delivery = Some(pending);
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
            super::restore_payload::resolve_restored_payload,
            |_, _, _| panic!("restored fixture must not fail"),
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
            |_| Ok(None),
            |_, _, _| panic!("restored fixture must not fail"),
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
    let authorization =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true);
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
        execution_mode: None,
        execution_sandbox: None,
        failed_provider: None,
        context_files: None,
        context_skills: None,
        worker_restart: None,
        captured: true,
        requested_model: Some("gpt-6-luna".into()),
        model: Some("gpt-6-luna".into()),
        requested_reasoning_effort: Some("xhigh".into()),
        resolved_reasoning_effort: Some("xhigh".into()),
        args: Some(vec!["--approval-mode".into(), "restricted".into()]),
        agent_name: Some("restore-agent".into()),
        identity_instructions: Some("opaque-instruction-fragment".into()),
        permission_bypass_requested: false,
        runtime_launch_options: None,
        structured_runtime_mode: None,
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
fn restored_snapshot_resumes_the_tab_owned_session_for_each_provider() {
    for provider in ["claude", "codex", "opencode", "grok"] {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new(provider).with_launch_binary(provider),
        ));
        let session_id = format!("{provider}-owned-session");
        let launch = RestoredLaunch {
            provider_id: provider.into(),
            session_id: Some(session_id.clone()),
            cwd: Some("fixture-cwd".into()),
            model: None,
            launch_parameters: RestorableLaunchParameters::default(),
            role: None,
            role_profile: None,
        };

        let input = restored_snapshot_launch_input(&registry, launch, None, None);
        let command =
            restored_snapshot_launch_command(&registry, "tab-provider-resume", provider, &input);

        assert_eq!(
            input.params.resume_session_id.as_deref(),
            Some(session_id.as_str())
        );
        assert!(
            command.contains(&format!("--resume {session_id}")),
            "{provider}"
        );
    }
}

#[test]
fn disabled_resume_uses_a_fresh_command_and_keeps_session_binding_history() {
    use crate::service::restored_launch::restored_resume_session_id;

    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("snapshot-marked")
            .with_launch_binary("snapshot-marked"),
    ));
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("snapshot-marked".into());
    tab.agent_session_id = Some("persisted-session".into());
    tab.session_binding_nonce = Some("prior-launch-nonce".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.restored_from_snapshot = true;
    let mux = Arc::new(TimedRwLock::new("disabled-resume-history", state));
    let session_id = restored_resume_session_id(false, Some("persisted-session"));
    assert_eq!(session_id, None);

    let launch = claim_restored_launch(
        &mux,
        &tab_id,
        "snapshot-marked",
        Some("persisted-session"),
        Some("fixture-cwd"),
        session_id.is_some(),
    )
    .unwrap()
    .expect("disabled resume claims a fresh launch");
    let input = restored_snapshot_launch_input(&registry, launch.clone(), None, None);
    let command = restored_snapshot_launch_command(&registry, &tab_id, "snapshot-marked", &input);
    let guard = mux.read();
    let tab = guard.get_tab(&tab_id).unwrap();

    assert_eq!(launch.session_id, None);
    assert!(!command.contains("--resume"));
    assert!(command.contains("--launch-marker"));
    assert!(tab.agent_session_id.is_none());
    assert_eq!(
        tab.prior_session_binding_nonces
            .last()
            .and_then(|entry| entry.session_id.as_deref()),
        Some("persisted-session")
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

#[cfg(test)]
mod restore_boot_stage_tests {
    use super::super::{reattach_boot_stage, restore_materialization_core};
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};
    use crate::service::boot_stage::{apply_boot_stage, BootStage};
    use codeterm_util::lock_hold::TimedRwLock;
    use sha2::Digest;
    use std::sync::Arc;

    struct RestoreFixture {
        _temp: tempfile::TempDir,
        store: crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore,
        mux: MuxState,
        tab_id: String,
        digest: String,
    }

    /// A restored agent tab whose pending-delivery payload is absent from the
    /// store, so the resolve step is guaranteed to fail.
    fn unresolvable_restored_tab(live_pty: bool) -> RestoreFixture {
        let digest = format!(
            "{:x}",
            sha2::Sha256::digest("missing restored fallback brief".as_bytes())
        );
        let temp = tempfile::tempdir().unwrap();
        let store = crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
            temp.path(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.cwd = Some("fixture-cwd".into());
        tab.restored_from_snapshot = true;
        tab.pty_id = live_pty.then(|| "live-pty".to_string());
        tab.pending_delivery = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));
        RestoreFixture {
            _temp: temp,
            store,
            mux: Arc::new(TimedRwLock::new("restore-boot-stage-test", state)),
            tab_id,
            digest,
        }
    }

    fn restore_with_failing_resolve(
        fixture: &RestoreFixture,
    ) -> (Result<(), String>, Vec<(BootStage, String)>, bool) {
        let emitted = parking_lot::Mutex::new(Vec::new());
        let launched = parking_lot::Mutex::new(false);
        let result = restore_materialization_core(
            &fixture.mux,
            &fixture.tab_id,
            "fixture-provider",
            None,
            Some("fixture-cwd"),
            false,
            |pending| {
                fixture
                    .store
                    .resolve(pending.unwrap())
                    .map(Some)
                    .map_err(|error| error.to_string())
            },
            |stage, reason, _| emitted.lock().push((stage, reason.to_string())),
            |_, _| *launched.lock() = true,
        );
        let stages = emitted.lock().clone();
        let launched = *launched.lock();
        (result.map(|_| ()), stages, launched)
    }

    #[test]
    fn restore_failure_on_live_shell_reports_agent_spawn_failed() {
        let fixture = unresolvable_restored_tab(true);
        let (result, stages, launched) = restore_with_failing_resolve(&fixture);

        assert_eq!(
            result.unwrap_err(),
            format!("RestorePayloadMissing: {}", fixture.digest),
            "the resolve error must still surface to the caller"
        );
        assert_eq!(
            stages,
            [(
                BootStage::SpawnFailed,
                format!("RestorePayloadMissing: {}", fixture.digest)
            )]
        );
        assert!(!launched, "a failed resolve must not launch an agent");
    }

    #[test]
    fn restore_failure_without_live_pty_still_reports_spawn_failed() {
        let fixture = unresolvable_restored_tab(false);
        let (result, stages, launched) = restore_with_failing_resolve(&fixture);

        assert!(result.is_err());
        assert_eq!(
            stages,
            vec![(
                BootStage::SpawnFailed,
                format!("RestorePayloadMissing: {}", fixture.digest)
            )],
            "with no live PTY a real spawn failure must stay distinguishable"
        );
        assert!(!launched);
    }

    #[test]
    fn proven_reattach_clears_a_latched_failure_without_restart() {
        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        let mut sequence = Vec::new();

        apply_boot_stage(
            tab,
            BootStage::SpawnFailed,
            Some("RestorePayloadMissing: deadbeef"),
        );
        sequence.push(tab.boot_stage.unwrap());
        assert_eq!(
            tab.boot_failure_reason.as_deref(),
            Some("RestorePayloadMissing: deadbeef"),
            "precondition: the Tab starts with a latched failure"
        );

        let reattached = reattach_boot_stage(tab.boot_stage, tab.boot_failure_reason.as_deref())
            .expect("reattach clears the latch");
        apply_boot_stage(tab, reattached, None);
        sequence.push(tab.boot_stage.unwrap());

        assert_eq!(
            sequence,
            vec![BootStage::SpawnFailed, BootStage::PtyReattached],
            "the UI-visible stage must transition out of the failure on reattach"
        );
        assert_eq!(
            tab.boot_failure_reason, None,
            "the latched failure reason must be cleared, not merely overdrawn"
        );
        assert!(
            reattach_boot_stage(tab.boot_stage, tab.boot_failure_reason.as_deref()).is_none(),
            "reattach is idempotent"
        );
    }

    #[test]
    fn reattach_preserves_restore_failures_that_explain_an_unavailable_agent() {
        for reason in [
            "restored_provider_unresolved: no unique owner",
            "restored_role_without_provider: no provider or session",
        ] {
            assert_eq!(
                reattach_boot_stage(Some(BootStage::SpawnFailed), Some(reason)),
                None,
                "a shell PTY reattach must not erase its agent restore failure"
            );
        }
    }

    #[test]
    fn reattach_never_rewrites_an_already_honest_stage() {
        for stage in [
            None,
            Some(BootStage::AgentReady),
            Some(BootStage::ShellReady),
            Some(BootStage::AgentLaunching),
            Some(BootStage::PtyReattached),
        ] {
            assert_eq!(
                reattach_boot_stage(stage, None),
                None,
                "reattach must not stomp an in-flight or ready boot: {stage:?}"
            );
        }
    }
}

#[test]
fn restored_snapshot_service_builder_keeps_direct_spec_and_owned_session() {
    use crate::plugin::js_runtime::{JsProvider, PluginVm};
    let (state, tab_id) = MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(state);
    let manifest = serde_json::json!({
        "id": "direct-restore-fixture", "version": "0.1.0", "displayName": "Fixture"
    });
    let source = r#"module.exports.default = {
        buildResumeCommand(sessionId) { return 'fixture --resume ' + sessionId; },
        buildLaunchSpec(ctx) {
            return {program: 'fixture', args: ['--resume', ctx.resume.sessionId]};
        }
    };"#;
    let vm = PluginVm::from_source("direct-restore-fixture", source.into()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            serde_json::from_value(manifest).unwrap(),
            Arc::new(vm),
        )));
    let input = restored_snapshot_launch_input(
        &fixture.service.agent_registry,
        RestoredLaunch {
            provider_id: "direct-restore-fixture".into(),
            session_id: Some("exact-own-session".into()),
            cwd: Some(fixture.temp.path().to_string_lossy().into_owned()),
            model: None,
            launch_parameters: RestorableLaunchParameters {
                captured: true,
                ..Default::default()
            },
            role: None,
            role_profile: None,
        },
        None,
        None,
    );
    let command = super::restored_snapshot_launch_command_for_service(
        &fixture.service,
        &tab_id,
        "direct-restore-fixture",
        &input,
    );
    let spec = command.direct.as_ref().unwrap();
    assert_eq!(spec.program, "fixture");
    assert_eq!(spec.args, ["--resume", "exact-own-session"]);
    let claim = fixture.service.pty_ownership.acquire(&tab_id);
    fixture
        .service
        .ensure_restored_launch_host_with_claim(&claim, &command, None)
        .unwrap();
    assert!(fixture.service.resolve_pty_id(&tab_id).is_err());
}

#[test]
fn restored_snapshot_shell_commands_match_the_service_builders_byte_for_byte() {
    use crate::plugin::js_runtime::{JsProvider, PluginVm};
    for session_id in [Some("owned-shell-session"), None] {
        let (state, tab_id) = MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(state);
        let registry = &fixture.service.agent_registry;
        let provider_id = "shell-restore-fixture";
        let manifest = serde_json::json!({
            "id": provider_id, "version": "0.1.0", "displayName": "Fixture"
        });
        let source = r#"module.exports.default = {
            buildLaunchCommand(params) { return 'fixture ' + JSON.stringify(params.args); },
            buildResumeCommand(sessionId) { return 'fixture --resume ' + sessionId; }
        };"#;
        let vm = PluginVm::from_source(provider_id, source.into()).unwrap();
        registry.register_provider(Box::new(JsProvider::new(
            serde_json::from_value(manifest).unwrap(),
            Arc::new(vm),
        )));
        assert_eq!(
            registry.with_provider(provider_id, |provider| provider.declares_launch_spec()),
            Some(false)
        );
        let input = restored_snapshot_launch_input(
            registry,
            RestoredLaunch {
                provider_id: provider_id.into(),
                session_id: session_id.map(str::to_owned),
                cwd: Some(fixture.temp.path().to_string_lossy().into_owned()),
                model: None,
                launch_parameters: RestorableLaunchParameters {
                    captured: true,
                    args: Some(vec![
                        "--opaque".into(),
                        "value with spaces".into(),
                        "$literal".into(),
                    ]),
                    ..Default::default()
                },
                role: None,
                role_profile: None,
            },
            None,
            None,
        );
        let prepared = super::restored_snapshot_launch_command_for_service(
            &fixture.service,
            &tab_id,
            provider_id,
            &input,
        );
        let expected = if session_id.is_some() {
            crate::service::orchestrator::build_agent_command_for_service(
                &fixture.service,
                &tab_id,
                provider_id,
                &input.params,
                input.launch_task.as_deref(),
                None,
                registry,
            )
        } else {
            crate::service::launch_session_detection::marked_agent_command_for_service(
                &fixture.service,
                &tab_id,
                provider_id,
                &input.params,
                input.launch_task.as_deref(),
                None,
                registry,
            )
        };
        crate::service::launch_session_detection::clear_launch_marker(&tab_id);
        assert!(prepared.direct.is_none());
        assert!(!expected.is_empty());
        assert_eq!(prepared.shell_command, expected);
    }
}
