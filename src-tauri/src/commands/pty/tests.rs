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
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

        for historical_task in [Some(HISTORICAL_TASK), None] {
            let input =
                restored_fresh_launch_input(&registry, worker_params("claude", historical_task));
            let command = input.command(&registry).expect("restored launch command");

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
}
