use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn tool_row(status: &str, args: &str, result: &str) -> ChatMsg {
    ChatMsg {
        id: "tool-row".into(),
        kind: "tool_call".into(),
        tool_status: Some(status.into()),
        tool_error: Some(status == "failed"),
        tool_args: Some(args.into()),
        tool_result: Some(result.into()),
        ..Default::default()
    }
}

#[test]
fn a_failed_agent_ack_row_names_its_cause_in_every_command_shape() {
    for args in [
        r#"{"command":["bash","-lc","domios agent ack"]}"#,
        "domios --json agent ack",
        r"C:\tools\domios.exe agent ack",
        "codeterm agent ack",
    ] {
        let outcome = ack_tool_outcome(&tool_row(
            "failed",
            args,
            "Error: connect socket\naccess denied",
        ))
        .unwrap_or_else(|| panic!("{args} runs the acknowledgement"));
        assert!(outcome.failed);
        assert!(
            outcome.cause.ends_with("access denied"),
            "{}",
            outcome.cause
        );
    }
    assert!(
        !ack_tool_outcome(&tool_row("failed", "domios agent ack", ""))
            .unwrap()
            .cause
            .is_empty()
    );
}

#[test]
fn other_commands_and_unsettled_rows_say_nothing_about_the_ack() {
    for row in [
        tool_row("failed", "domios agent report", "boom"),
        tool_row("failed", "agent ack", "boom"),
        tool_row("running", "domios agent ack", ""),
    ] {
        assert_eq!(ack_tool_outcome(&row), None);
    }
    let mut message = tool_row("failed", "domios agent ack", "boom");
    message.kind = "assistant".into();
    assert_eq!(ack_tool_outcome(&message), None);
}

#[test]
fn only_an_attempts_first_failure_reaches_the_parent_and_settled_output_is_kept_as_its_cause() {
    let registry = WorkerAckRegistry::default();
    let outcome = |failed, cause: &str| AckToolOutcome {
        failed,
        cause: cause.into(),
    };
    assert!(!registry.record_ack_outcome("tab", "attempt-a", &outcome(false, "settled")));
    assert_eq!(
        registry.ack_cause("tab", "attempt-a").as_deref(),
        Some("settled")
    );
    assert_eq!(registry.reported_ack_failure("tab"), None);

    assert!(registry.record_ack_outcome("tab", "attempt-a", &outcome(true, "denied")));
    assert!(!registry.record_ack_outcome("tab", "attempt-a", &outcome(true, "denied again")));
    assert_eq!(
        registry.reported_ack_failure("tab").as_deref(),
        Some("denied again")
    );
    assert_eq!(registry.ack_cause("tab", "attempt-b"), None);
    assert!(registry.record_ack_outcome("tab", "attempt-b", &outcome(true, "denied")));

    registry.clear_ack_evidence("tab");
    assert_eq!(registry.reported_ack_failure("tab"), None);
    assert_eq!(registry.ack_cause("tab", "attempt-b"), None);
}

fn task_bound_fixture(ack_recorded: bool) -> (ServiceFixture, String, String) {
    let (mut mux, parent) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&parent).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&parent).unwrap();
        tab.agent_type = Some("opaque-parent".into());
        tab.pty_id = Some("parent-pty".into());
    }
    mux.get_tab_mut(&worker).unwrap().agent_type = Some("opaque-provider".into());
    mux.get_tab_mut(&worker).unwrap().title = "Vesta".into();
    let fixture = ServiceFixture::new(mux);
    let mut plan = crate::plan::Plan::new(
        "opaque".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab(&parent),
    );
    let mut task: crate::plan::Task = serde_json::from_value(
        json!({"id":"task-a","name":"opaque","type":"code","status":"pending"}),
    )
    .unwrap();
    task.execution_attempts
        .push(crate::plan::TaskExecutionAttempt {
            id: "attempt-a".into(),
            executor_tab_id: worker.clone(),
            parent_tab_id: Some(parent.clone()),
            acknowledged_at: ack_recorded.then_some(1),
            ..Default::default()
        });
    plan.tasks.push(task);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    (fixture, parent, worker)
}

#[test]
fn a_failed_ack_reaches_the_parent_with_its_cause_and_shows_in_tab_status() {
    let (fixture, parent, worker) = task_bound_fixture(false);
    let outcome = AckToolOutcome {
        failed: true,
        cause: "access denied".into(),
    };
    fixture
        .service
        .record_worker_ack_tool_outcome(&worker, &outcome);
    fixture
        .service
        .record_worker_ack_tool_outcome(&worker, &outcome);

    let pending = fixture.service.report_store.lock().pending_for(&parent);
    assert_eq!(pending.len(), 1, "one report per failed attempt");
    assert_eq!(pending[0].task_id.as_deref(), Some("task-a"));
    for part in [
        "Vesta",
        "could not acknowledge task task-a",
        "attempt-a",
        "access denied",
    ] {
        assert!(pending[0].message.contains(part), "{}", pending[0].message);
    }
    let failure = fixture
        .service
        .get_tab_status(&worker)
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.code.as_deref(), Some(WORKER_ACK_FAILED));
    assert_eq!(failure.reason, "access denied");
    assert_eq!(
        fixture
            .service
            .worker_ack_failure_cause(&worker, "attempt-a")
            .as_deref(),
        Some("access denied"),
        "the startup deadline report carries the same cause"
    );
}

#[test]
fn an_acknowledged_attempt_is_never_reported_as_failed() {
    let (fixture, parent, worker) = task_bound_fixture(true);
    fixture.service.record_worker_ack_tool_outcome(
        &worker,
        &AckToolOutcome {
            failed: true,
            cause: "opaque".into(),
        },
    );
    assert!(fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent)
        .is_empty());
    assert!(fixture
        .service
        .get_tab_status(&worker)
        .unwrap()
        .failure
        .is_none());
}

#[test]
fn a_session_that_missed_its_full_access_tells_the_parent_and_is_the_deadline_cause() {
    let (fixture, parent, worker) = task_bound_fixture(false);
    let reason =
        "full_access_unavailable: declared mode opaque-stale not advertised (opaque-default)";
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker)
        .unwrap()
        .agent_runtime_state = Some(json!({"state":"loaded","epoch":1,"accessFailure":reason}));
    fixture
        .service
        .report_full_access_unavailable(&worker, reason);

    let pending = fixture.service.report_store.lock().pending_for(&parent);
    assert_eq!(pending.len(), 1);
    for part in ["did not get its declared full access", "opaque-stale"] {
        assert!(pending[0].message.contains(part), "{}", pending[0].message);
    }
    assert_eq!(
        fixture
            .service
            .worker_ack_failure_cause(&worker, "attempt-a")
            .as_deref(),
        Some(reason)
    );
    let failure = fixture
        .service
        .get_tab_status(&worker)
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.code.as_deref(), Some(FULL_ACCESS_UNAVAILABLE));
    assert_eq!(failure.reason, reason);
}
