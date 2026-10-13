//! Tests for the PTY spawn/kill commands and restored-snapshot launch input.

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
async fn protected_frontend_attach_reuses_the_backend_launch_claim() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().protected = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = std::sync::Arc::new(fixture.service.clone());
    let (backend, _) = svc
        .pty
        .lock()
        .spawn(None, None, 80, 24, Default::default())
        .unwrap();
    let claim = svc.pty_ownership.acquire(&tab_id);
    let selected = svc.link_pty_with_claim(&claim, &tab_id, backend.clone(), true);
    assert!(selected.linked);
    drop(claim);
    let attached = super::spawn_pty_core(
        &svc,
        None,
        None,
        80,
        24,
        Some("frontend-must-not-exist".into()),
        Some(tab_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(attached, backend);
    assert_eq!(
        svc.pty_ownership.launch_claim(&tab_id),
        Some(backend.clone())
    );
    assert!(svc
        .pty
        .lock()
        .get_session("frontend-must-not-exist")
        .is_none());
    svc.kill_ptys(&[backend]);
}

#[test]
fn frontend_restore_launch_arms_shared_readiness_with_snapshot_origin() {
    let source = include_str!("../pty.rs");
    let spawn = source
        .split("pub async fn spawn_pty_core(")
        .nth(1)
        .and_then(|remaining| remaining.split("pub async fn read_pty(").next())
        .expect("the frontend spawn command remains present");
    let call_sites = spawn.matches("dispatch_restored_snapshot_launch(").count()
        - spawn
            .matches("fn dispatch_restored_snapshot_launch(")
            .count();
    assert_eq!(call_sites, 2);
    assert!(!spawn.contains("poll_bare_shell_prompt"));
    assert!(!spawn.contains("await_agent_tui_frame_on_screen"));

    let dispatcher = source
        .split("fn dispatch_restored_snapshot_launch(")
        .nth(1)
        .expect("restored frontend launches use one dispatcher");
    assert!(dispatcher.contains("launch_agent_in_background_with_env("));
    assert!(dispatcher.contains("AgentLaunchOrigin::RestoredSnapshot"));

    let shared_launcher = include_str!("../../service/orchestrator/launch.rs");
    assert!(shared_launcher.contains("arm_with_origin(readiness_attempt.clone(), origin)"));
}

mod restored_fresh_launch_tests {
    use super::super::restored_fresh_launch_input;

    const HISTORICAL_TASK: &str = "historical assigned task";
    const RESTORED_INSTRUCTIONS: &str = "opaque-restored-instructions-61d9";

    fn worker_params(
        provider: &str,
        historical_task: Option<&str>,
    ) -> crate::service::SpawnAgentParams {
        crate::service::SpawnAgentParams {
            agent: Some(provider.into()),
            cwd: Some("fixture-cwd".into()),
            model: Some("fixture-model".into()),
            role: Some("Worker".into()),
            role_profile: Some("fixture-profile".into()),
            task: historical_task.map(str::to_string),
            system_prompt: Some(RESTORED_INSTRUCTIONS.into()),
            ..Default::default()
        }
    }

    #[test]
    fn restored_fresh_launch_preserves_saved_parameters_without_starter_turn() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture-provider")
                .with_session_config_delivery(),
        ));
        for historical_task in [Some(HISTORICAL_TASK), None] {
            let input = restored_fresh_launch_input(
                &registry,
                worker_params("fixture-provider", historical_task),
                crate::multiplexer::types::RestorableLaunchParameters {
                    context_files: None,
                    context_skills: None,
                    worker_restart: None,
                    captured: true,
                    identity_instructions: Some(RESTORED_INSTRUCTIONS.into()),
                    ..Default::default()
                },
            );

            assert_eq!(input.params.task, None);
            assert_eq!(input.params.role.as_deref(), Some("Worker"));
            assert_eq!(
                input.params.role_profile.as_deref(),
                Some("fixture-profile")
            );
            assert_eq!(input.params.cwd.as_deref(), Some("fixture-cwd"));
            assert_eq!(input.params.model.as_deref(), Some("fixture-model"));
            assert_eq!(
                input.params.starter_prompt,
                crate::agent::provider::StarterPrompt::NoStarter
            );
            assert_eq!(input.launch_task.as_deref(), Some(RESTORED_INSTRUCTIONS));
            assert!(!input
                .launch_task
                .as_deref()
                .unwrap()
                .contains(HISTORICAL_TASK));
        }
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn restored_fresh_launch_command_carries_the_supplied_instruction() {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::new().0,
        );
        let registry = &*fixture.service.agent_registry;
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

        for historical_task in [Some(HISTORICAL_TASK), None] {
            let input = restored_fresh_launch_input(
                registry,
                worker_params("claude", historical_task),
                crate::multiplexer::types::RestorableLaunchParameters {
                    context_files: None,
                    context_skills: None,
                    worker_restart: None,
                    captured: true,
                    identity_instructions: Some(RESTORED_INSTRUCTIONS.into()),
                    ..Default::default()
                },
            );
            let command = input
                .command(&fixture.service, "tab-restored-identity")
                .expect("restored launch command");

            assert_eq!(command.matches("--append-system-prompt-file").count(), 1);
            let path = command
                .rsplit_once("--append-system-prompt-file ")
                .expect("declared flag carries a path")
                .1
                .trim()
                .trim_matches(['\'', '"']);
            let delivered = std::fs::read_to_string(path).expect("prompt file the command names");
            let delivered = delivered.strip_prefix('\u{feff}').unwrap_or(&delivered);
            assert!(delivered.starts_with(RESTORED_INSTRUCTIONS));
            assert_eq!(delivered.matches(RESTORED_INSTRUCTIONS).count(), 1);
            let rule = crate::plugin::agent_claude::launch_prompt_rule().unwrap();
            assert_eq!(delivered.matches(rule.as_str()).count(), 1);
            assert!(!delivered.contains(HISTORICAL_TASK));
            assert!(command.contains("--model "));
        }
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn restored_fresh_launch_passes_a_minted_marker_and_resume_passes_none() {
        use crate::service::launch_session_detection::registered_launch_marker;

        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::new().0,
        );
        let registry = &*fixture.service.agent_registry;
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "restored-marked",
        )));

        let fresh = restored_fresh_launch_input(
            registry,
            worker_params("restored-marked", None),
            crate::multiplexer::types::RestorableLaunchParameters::default(),
        )
        .command(&fixture.service, "tab-restored-fresh")
        .expect("restored fresh launch command");
        let marker = registered_launch_marker("tab-restored-fresh")
            .expect("restored fresh launch registers its launch marker");
        assert!(!marker.is_empty());
        assert!(fresh.ends_with(&format!(" --launch-marker {marker}")));

        let mut resume_params = worker_params("restored-marked", None);
        resume_params.resume_session_id = Some("session-restored".into());
        let resumed = restored_fresh_launch_input(
            registry,
            resume_params,
            crate::multiplexer::types::RestorableLaunchParameters::default(),
        )
        .command(&fixture.service, "tab-restored-resume")
        .expect("restored resume command");
        assert!(!resumed.contains("--launch-marker"));
        assert_eq!(
            registered_launch_marker("tab-restored-resume").as_deref(),
            Some("session-restored")
        );
    }
}

mod spawn_pty_restore_tests {
    use std::sync::Arc;

    use super::super::spawn_pty_core;
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::RestorableLaunchParameters;
    use crate::service::test_fixture::ServiceFixture;

    const PROVIDER: &str = "restore-record";
    const SESSION: &str = "restore-record-session";
    const INSTRUCTIONS: &str = "opaque-restore-system-input-c531";

    fn restored_fixture() -> (
        ServiceFixture,
        String,
        Arc<parking_lot::Mutex<Vec<crate::agent::provider::AgentResumeParams>>>,
    ) {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some(SESSION.into());
        tab.restored_from_snapshot = true;
        tab.protected = false;
        tab.pty_id = Some("restore-pty-not-yet-materialized".into());
        tab.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);

        let fixture = ServiceFixture::new(mux);
        fixture.set_agent_full_access(true);
        use_fixture_dir_as_tab_cwd(&fixture, &tab_id);
        let resume_context = Arc::new(parking_lot::Mutex::new(Vec::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::with_session(PROVIDER, SESSION)
                .with_session_config_delivery()
                .with_resume_context_log(resume_context.clone()),
        ));
        (fixture, tab_id, resume_context)
    }

    /// A restored launch refuses a cwd that is not a directory, so the fixture's
    /// own directory stands in for the saved project.
    fn use_fixture_dir_as_tab_cwd(fixture: &ServiceFixture, tab_id: &str) {
        fixture.service.mux.write().get_tab_mut(tab_id).unwrap().cwd =
            Some(fixture.temp.path().display().to_string());
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn spawn_pty_core_restore_branch_builds_record_and_selects_first_turn_readiness() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some(SESSION.into());
        tab.restored_from_snapshot = true;
        tab.restorable_launch = RestorableLaunchParameters {
            failed_provider: None,
            structured_runtime_mode: None,
            context_files: None,
            context_skills: None,
            worker_restart: None,
            captured: true,
            requested_model: Some("gpt-6-luna".into()),
            model: Some("gpt-6-luna".into()),
            requested_reasoning_effort: Some("xhigh".into()),
            resolved_reasoning_effort: Some("xhigh".into()),
            args: Some(vec!["--opaque-launch-arg".into(), "restored-value".into()]),
            agent_name: Some("restored-agent-name".into()),
            identity_instructions: Some(INSTRUCTIONS.into()),
            permission_bypass_requested: false,
        };

        let fixture = ServiceFixture::new(mux);
        use_fixture_dir_as_tab_cwd(&fixture, &tab_id);
        let resume_context = Arc::new(parking_lot::Mutex::new(Vec::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::with_session(PROVIDER, SESSION)
                .with_session_config_delivery()
                .with_session_starts_on_first_user_turn()
                .with_models(vec![crate::agent::provider::ModelInfo {
                    id: "gpt-6-luna".into(),
                    display_name: "gpt-6-luna".into(),
                    reasoning_efforts: Some(vec![crate::agent::provider::ReasoningEffortInfo {
                        id: "xhigh".into(),
                        display_name: "xhigh".into(),
                        description: None,
                    }]),
                    ..Default::default()
                }])
                .with_resume_context_log(resume_context.clone()),
        ));

        let service = Arc::new(fixture.service.clone());
        let pty_id = spawn_pty_core(&service, None, None, 80, 24, None, Some(tab_id.clone()))
            .await
            .expect("restore PTY spawn succeeds");
        let built = resume_context
            .lock()
            .first()
            .cloned()
            .expect("spawn_pty_core reaches the contextual resume builder");

        assert_eq!(built.session_id, SESSION);
        assert_eq!(built.agent_name.as_deref(), Some("restored-agent-name"));
        assert_eq!(built.system_prompt.as_deref(), Some(INSTRUCTIONS));
        assert!(built.instance_id.is_some());
        assert_eq!(
            built.args,
            vec![
                "--opaque-launch-arg".to_string(),
                "restored-value".to_string(),
                "--model".to_string(),
                "gpt-6-luna".to_string(),
                "--effort".to_string(),
                "xhigh".to_string(),
            ]
        );
        assert!(!built.skip_permissions);

        let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("gpt-6-luna")
        );
        assert_eq!(
            tab.reasoning_effort_identity.resolved.as_deref(),
            Some("xhigh")
        );
        assert!(!tab.auto_approve);
        assert!(crate::service::launch_waits_for_first_turn_binding(
            &fixture.service,
            &tab_id,
            PROVIDER,
            true,
            crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
        ));

        let mut pty = fixture.service.pty.lock();
        pty.kill(&pty_id).expect("test PTY cleanup");
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn restored_agent_relaunches_once_when_ensure_pty_activates_first() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        assert_eq!(
            crate::agent::live_state::observe_tab_liveness(&fixture.service, &tab_id),
            codeterm_types::pty::AgentLiveness::Absent
        );
        assert!(!fixture
            .service
            .reconcile_tab_restored_without_agent(&tab_id));
        fixture
            .service
            .ensure_tab_pty_for_send(&tab_id)
            .expect("backend materialization activates and resumes the restored tab");
        assert_eq!(resume_context.lock().len(), 1);

        let pty_id = spawn_pty_core(
            &Arc::new(fixture.service.clone()),
            None,
            None,
            80,
            24,
            None,
            Some(tab_id.clone()),
        )
        .await
        .expect("frontend attach reuses the activated PTY");

        assert_eq!(resume_context.lock().len(), 1);
        let mut pty = fixture.service.pty.lock();
        pty.kill(&pty_id).expect("test PTY cleanup");
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn unobserved_liveness_then_ensure_pty_activation_schedules_restore_relaunch() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = None;
            tab.boot_stage = None;
        }
        assert_eq!(
            crate::agent::live_state::observe_tab_liveness(&fixture.service, &tab_id),
            codeterm_types::pty::AgentLiveness::Unobserved
        );

        fixture
            .service
            .ensure_tab_pty_for_send(&tab_id)
            .expect("ensure-pty activates and materializes the restored agent");

        assert_eq!(resume_context.lock().len(), 1);
        let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert!(!tab.restored_from_snapshot);
        let mut pty = fixture.service.pty.lock();
        pty.kill(tab.pty_id.as_deref().unwrap())
            .expect("test PTY cleanup");
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn restored_agent_relaunches_once_when_frontend_attaches_first() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        let backend_pty_id = "backend-materialized-restored-pty";
        fixture
            .service
            .pty
            .lock()
            .spawn_with_id(
                Some(backend_pty_id.into()),
                None,
                None,
                80,
                24,
                Default::default(),
            )
            .expect("backend materialization creates a live PTY");
        {
            let mut mux = fixture.service.mux.write();
            mux.link_pty(&tab_id, backend_pty_id.into());
        }
        let service = Arc::new(fixture.service.clone());
        let pty_id = spawn_pty_core(&service, None, None, 80, 24, None, Some(tab_id.clone()))
            .await
            .expect("frontend materialization activates and resumes the restored tab");
        assert_eq!(resume_context.lock().len(), 1);
        assert_eq!(pty_id, backend_pty_id);

        fixture
            .service
            .ensure_tab_pty_for_send(&tab_id)
            .expect("backend materialization reuses the frontend PTY");

        assert_eq!(resume_context.lock().len(), 1);
        let mut pty = fixture.service.pty.lock();
        pty.kill(&pty_id).expect("test PTY cleanup");
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn spawn_pty_live_restored_reuse_returns_after_materialization() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        let pty_guard = fixture.bind_test_pty_session(&tab_id, SESSION, "restored-reuse-nonce");
        let backend = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pty_id
            .clone()
            .unwrap();
        let service = Arc::new(fixture.service.clone());
        let target = tab_id.clone();
        let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
        let operation = tokio::spawn(async move {
            let result = spawn_pty_core(&service, None, None, 80, 24, None, Some(target)).await;
            completed_tx.send(result).unwrap();
        });
        assert_eq!(completed_rx.await.unwrap().unwrap(), backend);
        operation.await.unwrap();
        assert_eq!(resume_context.lock().len(), 1);
        let restored = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert!(!restored.restored_from_snapshot);
        assert_eq!(restored.pty_id.as_deref(), Some(backend.as_str()));
        drop(pty_guard);
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn manual_restart_claim_prevents_frontend_remount_restore_launch() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        let pty_id = "manual-restart-restored-pty";
        fixture
            .service
            .pty
            .lock()
            .spawn_with_id(Some(pty_id.into()), None, None, 80, 24, Default::default())
            .expect("manual restart owns a live PTY");
        {
            let mut mux = fixture.service.mux.write();
            mux.link_pty(&tab_id, pty_id.into());
        }
        crate::service::mark_tab_agent_launch_started(&fixture.service.mux, &tab_id);

        let attached = spawn_pty_core(
            &Arc::new(fixture.service.clone()),
            None,
            None,
            80,
            24,
            None,
            Some(tab_id.clone()),
        )
        .await
        .expect("frontend remount reuses the manual-restart PTY");

        assert_eq!(attached, pty_id);
        assert!(resume_context.lock().is_empty());
        let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert!(!tab.restored_from_snapshot);
        assert!(tab.agent_booting);
        let mut pty = fixture.service.pty.lock();
        pty.kill(pty_id).expect("test PTY cleanup");
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn frontend_attach_keeps_live_pty_when_restore_materialization_fails() {
        let (fixture, tab_id, resume_context) = restored_fixture();
        let pty_id = "failed-materialization-live-pty";
        fixture
            .service
            .pty
            .lock()
            .spawn_with_id(Some(pty_id.into()), None, None, 80, 24, Default::default())
            .expect("backend already owns a live PTY");
        {
            let mut mux = fixture.service.mux.write();
            mux.link_pty(&tab_id, pty_id.into());
            mux.get_tab_mut(&tab_id).unwrap().pending_delivery =
                Some(crate::multiplexer::types::PendingDeliveryRecord::new(
                    "missing-payload-digest".into(),
                    crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                ));
        }

        let attached = spawn_pty_core(
            &Arc::new(fixture.service.clone()),
            None,
            None,
            80,
            24,
            None,
            Some(tab_id),
        )
        .await
        .expect("restore failure must not discard a live PTY attachment");

        assert_eq!(attached, pty_id);
        assert!(resume_context.lock().is_empty());
        let mut pty = fixture.service.pty.lock();
        pty.kill(pty_id).expect("test PTY cleanup");
    }
}

mod restored_scrollback_tests {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use std::sync::Arc;

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
    async fn frontend_first_spawn_seeds_history_before_reader_start() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tail = b"restored frontend spawn history\r\n";
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.scrollback_seed_pending = true;
        tab.pending_scrollback_tail = Some(Arc::from(tail.to_vec()));
        let fixture = ServiceFixture::new(mux);
        let service = Arc::new(fixture.service.clone());

        let pty_id = crate::commands::pty::spawn_pty_core(
            &service,
            None,
            None,
            80,
            24,
            None,
            Some(tab_id.clone()),
        )
        .await
        .expect("frontend spawn should materialize the restored PTY");

        let buffer = service.output_buf.lock();
        assert!(buffer.get_full(&pty_id).unwrap().starts_with(tail));
        assert_eq!(
            buffer.read_since(&pty_id, 0).0.get(..tail.len()),
            Some(tail.as_slice())
        );
        drop(buffer);
        service.kill_ptys(&[pty_id]);
    }
}

#[tokio::test]
async fn live_restored_reuse_transfers_its_claim_and_completes_on_materialization() {
    use crate::service::pty_ownership::{LinkOutcome, OwnershipEvent, PtyOwnership};
    use std::sync::{Arc, Mutex};

    let ownership = PtyOwnership::default();
    ownership.set_test_observer(Arc::new(|_, event| {
        if event == OwnershipEvent::AwaitingActivation {
            panic!("the reuse operation reacquired its activation permit");
        }
    }));
    let claim = ownership
        .acquire_async("restored-reuse".into())
        .await
        .unwrap();
    let linked = Arc::new(Mutex::new(None::<String>));
    let link_state = linked.clone();
    let (materialized_tx, materialized_rx) = tokio::sync::oneshot::channel();
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let operation = tokio::spawn(async move {
        let result = super::reuse_live_pty_with_claim(
            claim,
            "live-pty".into(),
            true,
            move |claim, existing| {
                claim.settle(
                    existing,
                    false,
                    |id| id == "live-pty",
                    |id| LinkOutcome::Linked(link_state.lock().unwrap().replace(id.into())),
                    |_| panic!("the live PTY must remain owned"),
                    |_| {},
                )
            },
            move |claim| {
                assert_eq!(claim.tab_id(), "restored-reuse");
                materialized_tx.send(()).unwrap();
                Ok(())
            },
        )
        .await;
        completed_tx.send(result).unwrap();
    });
    materialized_rx.await.unwrap();
    assert_eq!(completed_rx.await.unwrap().unwrap(), "live-pty");
    operation.await.unwrap();
    assert_eq!(linked.lock().unwrap().as_deref(), Some("live-pty"));
    let next_claim = ownership
        .acquire_async("restored-reuse".into())
        .await
        .unwrap();
    assert_eq!(next_claim.tab_id(), "restored-reuse");
}
