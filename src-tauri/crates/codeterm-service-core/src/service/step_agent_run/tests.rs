use super::*;

#[test]
fn registered_run_prompt_steps_have_valid_hidden_tab_titles() {
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut checked = 0;
    for entry in std::fs::read_dir(plugins).unwrap() {
        let path = entry.unwrap().path().join("plugin.json");
        if !path.is_file() {
            continue;
        }
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let Some(steps) = manifest
            .pointer("/capabilities/graphNodes/steps")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for step in steps {
            if !step["id"]
                .as_str()
                .is_some_and(|id| id.ends_with(".run_prompt"))
            {
                continue;
            }
            let label = step["title"].as_str().unwrap();
            let title = step_agent_title(label);
            crate::service::spawn_title::validate_agent_tab_title(
                Some(&title),
                manifest["id"].as_str(),
            )
            .unwrap();
            assert_eq!(title, "Prompt");
            checked += 1;
        }
    }
    assert_eq!(checked, 3);
}

fn message(kind: &str, content: &str) -> ChatMsg {
    serde_json::from_value(serde_json::json!({"id":content,"type":kind,"content":content})).unwrap()
}

#[test]
fn step_run_reply_is_the_final_assistant_message_and_is_bounded() {
    let turn = [message("user", "ask"), message("assistant", " answer ")];
    assert_eq!(final_reply(&turn).as_deref(), Some("answer"));
    assert_eq!(
        final_reply(&[message("assistant", "a"), message("tool_result", "x")]),
        None
    );
    assert_eq!(final_reply(&[message("assistant", "  ")]), None);
    let long = "é".repeat(MAX_REPLY_BYTES);
    let reply = final_reply(&[message("assistant", &long)]).unwrap();
    assert!(reply.len() <= MAX_REPLY_BYTES && reply.ends_with(TRUNCATED));
}

#[test]
fn step_run_cwd_refuses_relative_missing_and_file_paths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(working_dir("relative/dir").is_err());
    assert!(working_dir(&dir.path().join("missing").to_string_lossy()).is_err());
    assert!(working_dir(&file.to_string_lossy()).is_err());
    let shown = working_dir(&dir.path().to_string_lossy()).unwrap();
    assert!(!shown.starts_with(r"\\?\"));
}

#[test]
fn step_run_unknown_and_foreign_operations_never_replay_or_cancel() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let run_id = uuid::Uuid::new_v4().to_string();
    RUNS.runs.lock().insert(
        run_id.clone(),
        StepRun {
            plugin: "fixture".into(),
            operation: "own-op".into(),
            tab_id: None,
            state: StepRunState::Running,
            ended_at: None,
        },
    );
    assert_eq!(
        fixture
            .service
            .wait_step_agent_run("fixture", "foreign-op", &run_id, Instant::now()),
        StepRunState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .cancel_step_agent_run("fixture", "foreign-op", &run_id),
        StepRunState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .cancel_step_agent_run("foreign", "own-op", &run_id),
        StepRunState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .wait_step_agent_run("fixture", "own-op", "lost", Instant::now()),
        StepRunState::Unknown
    );
    assert_eq!(
        fixture
            .service
            .cancel_step_agent_run("fixture", "own-op", &run_id),
        StepRunState::Canceled
    );
    RUNS.runs.lock().remove(&run_id);
}

#[test]
fn step_run_access_follows_global_host_policy_and_confines_sandbox() {
    struct PermissionFixture;
    impl crate::agent::provider::AgentProvider for PermissionFixture {
        fn id(&self) -> &str {
            "permission-fixture"
        }
        fn display_name(&self) -> &str {
            "Permission fixture"
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
            String::new()
        }
        fn auto_approve_flags(&self) -> Vec<String> {
            vec!["--fixture-grant".into()]
        }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            crate::agent::provider::ReadinessStrategy::PollShellPrompt {
                timeout: std::time::Duration::from_secs(1),
            }
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            crate::agent::provider::TaskDelivery::InlineFlag
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn detect_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            None
        }
    }
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.set_agent_full_access(true);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(PermissionFixture));
    assert!(
        step_permission_bypass(&fixture.service, "permission-fixture", false)
            .unwrap()
            .is_some()
    );
    assert!(
        step_permission_bypass(&fixture.service, "permission-fixture", true)
            .unwrap()
            .is_none()
    );
    fixture.set_agent_full_access(false);
    assert!(
        step_permission_bypass(&fixture.service, "permission-fixture", false)
            .unwrap()
            .is_none()
    );
    let wire = SpawnAgentParams::from_json(&serde_json::json!({"agent":"permission-fixture","skip_permissions":true,"permission_bypass_authorization":true})).unwrap();
    assert_eq!(wire.skip_permissions, None);
    assert!(!wire.permission_bypass_requested("permission-fixture"));
    assert!(
        step_permission_bypass(&fixture.service, "permission-fixture", false)
            .unwrap()
            .is_none()
    );
}

#[test]
fn step_run_terminal_completion_always_closes_its_tab() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab = mux.create_tab(&workspace).unwrap();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let id = uuid::Uuid::new_v4().to_string();
    RUNS.runs.lock().insert(
        id.clone(),
        StepRun {
            plugin: "fixture".into(),
            operation: "own".into(),
            tab_id: Some(tab.clone()),
            state: StepRunState::Running,
            ended_at: None,
        },
    );
    finish(
        &fixture.service,
        &id,
        true,
        String::new(),
        Ok(vec![message("assistant", "opaque reply")]),
    );
    assert!(fixture.service.mux.read().get_tab(&tab).is_none());
    assert_eq!(
        fixture
            .service
            .wait_step_agent_run("fixture", "own", &id, Instant::now()),
        StepRunState::Done {
            text: "opaque reply".into()
        }
    );
    RUNS.runs.lock().remove(&id);
}

#[test]
fn step_run_completion_before_wait_retains_exact_outcome_and_settles_once() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let id = uuid::Uuid::new_v4().to_string();
    RUNS.runs.lock().insert(
        id.clone(),
        StepRun {
            plugin: "fixture".into(),
            operation: "own".into(),
            tab_id: None,
            state: StepRunState::Running,
            ended_at: None,
        },
    );
    finish(
        &fixture.service,
        &id,
        true,
        String::new(),
        Ok(vec![message("assistant", "opaque final")]),
    );
    assert!(settle(&id, StepRunState::Canceled).is_none());
    for (plugin, operation, run) in [
        ("foreign", "own", id.as_str()),
        ("fixture", "foreign", id.as_str()),
        ("fixture", "own", "foreign"),
    ] {
        assert_eq!(
            fixture
                .service
                .wait_step_agent_run(plugin, operation, run, Instant::now()),
            StepRunState::Unknown
        );
    }
    for _ in 0..2 {
        assert_eq!(
            fixture
                .service
                .wait_step_agent_run("fixture", "own", &id, Instant::now()),
            StepRunState::Done {
                text: "opaque final".into()
            }
        );
    }
    RUNS.runs.lock().remove(&id);
}

#[test]
fn step_run_cancel_before_completion_retains_cancel_without_replay() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let id = uuid::Uuid::new_v4().to_string();
    RUNS.runs.lock().insert(
        id.clone(),
        StepRun {
            plugin: "fixture".into(),
            operation: "own".into(),
            tab_id: None,
            state: StepRunState::Running,
            ended_at: None,
        },
    );
    assert_eq!(
        fixture.service.cancel_step_agent_run("fixture", "own", &id),
        StepRunState::Canceled
    );
    finish(
        &fixture.service,
        &id,
        true,
        String::new(),
        Ok(vec![message("assistant", "late final")]),
    );
    assert_eq!(
        fixture
            .service
            .wait_step_agent_run("fixture", "own", &id, Instant::now()),
        StepRunState::Canceled
    );
    RUNS.runs.lock().remove(&id);
}
