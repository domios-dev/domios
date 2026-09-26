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

    const WORKER_IDENTITY_MARKER: &str = "You are a CodeTerm Worker.";
    const HISTORICAL_TASK: &str = "historical assigned task";

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
            ..Default::default()
        }
    }

    #[test]
    fn restored_fresh_launch_preserves_worker_identity_without_starter_turn() {
        let registry = crate::agent::registry::AgentRegistry::new();
        for historical_task in [Some(HISTORICAL_TASK), None] {
            let input = restored_fresh_launch_input(
                &registry,
                worker_params("fixture-provider", historical_task),
            );
            let identity = input.launch_task.as_deref().unwrap();

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
            assert_eq!(identity.matches(WORKER_IDENTITY_MARKER).count(), 1);
            assert!(!identity.contains("## Assignment"));
            assert!(!identity.contains(HISTORICAL_TASK));
        }
    }

    /// The struct assertions above stop one call short of the command, which is
    /// where the identity was being discarded. A registered provider is what
    /// makes the command reachable at all — an unregistered id returns None.
    #[test]
    fn restored_fresh_launch_command_carries_the_identity_exactly_once() {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::new().0,
        );
        let registry = &*fixture.service.agent_registry;
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

        for historical_task in [Some(HISTORICAL_TASK), None] {
            let input =
                restored_fresh_launch_input(registry, worker_params("claude", historical_task));
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
            assert_eq!(delivered.matches(WORKER_IDENTITY_MARKER).count(), 1);
            assert_eq!(delivered.matches(HISTORICAL_TASK).count(), 0);
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

        let fresh = restored_fresh_launch_input(registry, worker_params("restored-marked", None))
            .command(&fixture.service, "tab-restored-fresh")
            .expect("restored fresh launch command");
        let marker = registered_launch_marker("tab-restored-fresh")
            .expect("restored fresh launch registers its launch marker");
        assert!(!marker.is_empty());
        assert!(fresh.ends_with(&format!(" --launch-marker {marker}")));

        let mut resume_params = worker_params("restored-marked", None);
        resume_params.resume_session_id = Some("session-restored".into());
        let resumed = restored_fresh_launch_input(registry, resume_params)
            .command(&fixture.service, "tab-restored-resume")
            .expect("restored resume command");
        assert!(!resumed.contains("--launch-marker"));
        assert_eq!(registered_launch_marker("tab-restored-resume"), None);
    }
}
