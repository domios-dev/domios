//! Tests for the PTY spawn/kill commands and restored-snapshot launch input.

mod kill_pty_tests {
    use std::collections::HashMap;

    use super::super::{is_missing_pty_error, kill_pty_idempotent};

    #[test]
    fn kill_pty_idempotent_kills_child_and_allows_repeat() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let (pty_id, session) = manager
            .spawn(None, None, 80, 24, HashMap::new())
            .expect("spawn default shell");
        let child_pid = session.child.lock().expect("child lock").process_id();
        assert!(child_pid.is_some(), "spawned child should expose a pid");

        kill_pty_idempotent(&mut manager, &pty_id).expect("first kill succeeds");
        assert!(
            manager.get_session(&pty_id).is_none(),
            "kill must remove session from manager"
        );
        // TerminateProcess is async on Windows and the reaper thread may reap
        // concurrently; poll until the child is gone (here or in background).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match session.child.lock().expect("child lock").try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "kill must terminate the child"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        kill_pty_idempotent(&mut manager, &pty_id).expect("repeat kill is a no-op");
        kill_pty_idempotent(&mut manager, "missing-pty").expect("unknown kill is a no-op");
    }

    #[test]
    fn kill_pty_idempotent_does_not_swallow_unrelated_errors() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let err = manager
            .kill("real-id")
            .expect_err("manager should expose exact missing-id error");
        assert!(is_missing_pty_error("real-id", &err));
        assert!(!is_missing_pty_error("other-id", &err));
        assert!(!is_missing_pty_error(
            "real-id",
            "PTY real-id not found: access denied"
        ));
        assert!(!is_missing_pty_error(
            "real-id",
            "wrapped PTY real-id not found"
        ));
    }
}

mod payload_preflight_tests {
    use super::super::deliver_composed_text;
    use crate::service::delivery_error::{
        preflight_payload_size, SendOrigin, PROGRAMMATIC_SEND_MAX_BYTES,
    };

    /// Runs the seam with spy closures; returns (result, submit lengths seen,
    /// insert lengths seen) so refusal proves zero delivery-path invocation.
    fn run_spied(origin: SendOrigin, data: &str) -> (Result<(), String>, Vec<usize>, Vec<usize>) {
        let mut sends = Vec::new();
        let mut inserts = Vec::new();
        let result = deliver_composed_text(
            origin,
            "pty-1",
            data,
            |text| {
                sends.push(text.len());
                Ok(())
            },
            |text| {
                inserts.push(text.len());
                Ok(())
            },
        );
        (result, sends, inserts)
    }

    #[test]
    fn command_path_payload_preflight_refuses_over_bound_programmatic_without_delivery() {
        let payload = "a".repeat(PROGRAMMATIC_SEND_MAX_BYTES + 1);
        let (result, sends, inserts) = run_spied(SendOrigin::Programmatic, &payload);
        // Round-trip through the same typed constructor — never quoted prose.
        let expected =
            preflight_payload_size("pty-1", SendOrigin::Programmatic, payload.len()).unwrap_err();
        assert_eq!(result.unwrap_err(), expected.ledger_message());
        assert!(sends.is_empty(), "refused send must never reach delivery");
        assert!(inserts.is_empty());
    }

    #[test]
    fn command_path_payload_preflight_passes_at_bound_programmatic_send() {
        // Trailing \r counts toward the bound and still routes to submit.
        let payload = format!("{}\r", "a".repeat(PROGRAMMATIC_SEND_MAX_BYTES - 1));
        let (result, sends, inserts) = run_spied(SendOrigin::Programmatic, &payload);
        result.unwrap();
        assert_eq!(sends, [PROGRAMMATIC_SEND_MAX_BYTES - 1]);
        assert!(inserts.is_empty());
    }

    #[test]
    fn command_path_payload_preflight_exempts_user_origin_over_bound_send() {
        let payload = "a".repeat(PROGRAMMATIC_SEND_MAX_BYTES + 1);
        let (result, sends, inserts) = run_spied(SendOrigin::User, &payload);
        result.unwrap();
        assert!(sends.is_empty());
        assert_eq!(inserts, [PROGRAMMATIC_SEND_MAX_BYTES + 1]);
    }
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
            assert_eq!(delivered, RESTORED_INSTRUCTIONS);
            assert!(!delivered.contains(HISTORICAL_TASK));
            assert!(command.contains("--model "));
        }
    }

    #[test]
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
        assert_eq!(registered_launch_marker("tab-restored-resume"), None);
    }
}

mod spawn_pty_restore_tests {
    use std::sync::Arc;

    use super::super::{kill_pty_idempotent, spawn_pty_core};
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
        tab.cwd = Some("restore-cwd".into());
        tab.restored_from_snapshot = true;
        tab.protected = false;
        tab.pty_id = Some("restore-pty-not-yet-materialized".into());
        tab.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);

        let fixture = ServiceFixture::new(mux);
        let resume_context = Arc::new(parking_lot::Mutex::new(Vec::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::with_session(PROVIDER, SESSION)
                .with_session_config_delivery()
                .with_resume_context_log(resume_context.clone()),
        ));
        (fixture, tab_id, resume_context)
    }

    #[tokio::test]
    #[cfg_attr(not(windows), ignore = "requires desktop runtime")]
    async fn spawn_pty_core_restore_branch_builds_record_and_selects_first_turn_readiness() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some(SESSION.into());
        tab.cwd = Some("restore-cwd".into());
        tab.restored_from_snapshot = true;
        tab.restorable_launch = RestorableLaunchParameters {
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
        kill_pty_idempotent(&mut pty, &pty_id).expect("test PTY cleanup");
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
        kill_pty_idempotent(&mut pty, &pty_id).expect("test PTY cleanup");
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
        kill_pty_idempotent(&mut pty, tab.pty_id.as_deref().unwrap())
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
        let pty_id = spawn_pty_core(
            &service,
            None,
            None,
            80,
            24,
            None,
            Some(tab_id.clone()),
        )
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
        kill_pty_idempotent(&mut pty, &pty_id).expect("test PTY cleanup");
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
            .spawn_with_id(
                Some(pty_id.into()),
                None,
                None,
                80,
                24,
                Default::default(),
            )
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
        kill_pty_idempotent(&mut pty, pty_id).expect("test PTY cleanup");
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
            .spawn_with_id(
                Some(pty_id.into()),
                None,
                None,
                80,
                24,
                Default::default(),
            )
            .expect("backend already owns a live PTY");
        {
            let mut mux = fixture.service.mux.write();
            mux.link_pty(&tab_id, pty_id.into());
            mux.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(
                crate::multiplexer::types::PendingDeliveryRecord::new(
                    "missing-payload-digest".into(),
                    crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                ),
            );
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
        kill_pty_idempotent(&mut pty, pty_id).expect("test PTY cleanup");
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
