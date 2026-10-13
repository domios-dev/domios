use super::*;
#[test]
fn startup_notice_exposes_inspect_respond_and_hold_without_task_resend() {
    let event = agent_startup_notice_event("child", "Planner", "launch-a");
    assert_eq!(event["kind"], "agent_startup_overdue");
    assert_eq!(
        event["deadline_seconds"],
        AGENT_STARTUP_NOTICE_TIMEOUT.as_secs()
    );
    assert_eq!(event["tab_state"], TAB_STATE_ALIVE_AWAITING_INSPECTION);
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert_eq!(event["recovery"]["target_tab_id"], "child");
    assert_eq!(event["recovery"]["attempt_id"], "launch-a");
    assert_eq!(
        event["hold"],
        "codeterm agent startup-hold --tab child --attempt launch-a"
    );
    assert!(event["inspect"].as_array().unwrap().iter().any(|command| {
        command
            .as_str()
            .is_some_and(|command| command.contains("tab get-text"))
    }));
    assert!(event.get("deadline_basis").is_some());
}

#[test]
fn startup_overdue_notice_is_suppressed_after_binding_is_observed() {
    let (fixture, parent_tab_id, child_tab_id) = lifecycle_fixture(true);
    let authority = StartupAuthority::new(&parent_tab_id, &child_tab_id, "launch-a")
        .for_binding(Some("session-a"), "nonce-a");
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&child_tab_id).expect("child tab");
        tab.pty_id = Some("launch-a".into());
        tab.agent_session_id = Some("session-a".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
    }

    assert!(fixture.service.startup_attempt_binding_observed(&authority));

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&child_tab_id)
        .unwrap()
        .reported_session_binding_nonce = None;
    assert!(!fixture.service.startup_attempt_binding_observed(&authority));
}

#[test]
fn expired_startup_reports_a_surviving_tab_without_cleanup_schema() {
    let event = agent_startup_failed_event(
        "child",
        "Planner",
        "launch-a",
        Some(17),
        Some("synthetic provider rejection"),
        None,
    );
    assert_eq!(event["kind"], "agent_startup_failed");
    assert_eq!(
        event["deadline_seconds"],
        (AGENT_STARTUP_NOTICE_TIMEOUT + AGENT_STARTUP_FINAL_TIMEOUT).as_secs()
    );
    assert_eq!(event["tab_state"], TAB_STATE_ALIVE_AWAITING_INSPECTION);
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert!(event.get("cleanup").is_none());
    assert!(event.get("cleanup_error").is_none());
    assert_eq!(event["provider"]["exit_code"], 17);
    assert_eq!(event["provider"]["output"], "synthetic provider rejection");
}

#[test]
fn worker_ack_notice_is_a_short_single_line_with_agent_provider_and_attempt() {
    let notice = worker_lifecycle_notice(
        "Otto",
        Some("opencode"),
        None,
        &json!({
            "kind": "worker_ack",
            "task_id": "task_g94uu5",
            "attempt_id": "attempt_rjd7ab",
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(!notice.starts_with('{'));
    assert!(notice.contains("Otto"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("task_g94uu5"));
    assert!(notice.contains("attempt_rjd7ab"));
    assert!(notice.len() < 180);
}

#[test]
fn startup_notice_treats_awaiting_owner_prompt_as_waiting() {
    let payload = json!({
        "kind": "agent_startup_overdue",
        "attempt_id": "attempt-launch",
        "waiting_for_owner_prompt": true,
    });
    assert!(startup_notice_waits_for_owner(&payload));
}

#[test]
fn provider_startup_failure_is_in_the_single_line_notice() {
    let notice = worker_lifecycle_notice(
        "Nora",
        Some("opencode"),
        None,
        &json!({
            "kind": "agent_startup_failed",
            "attempt_id": "attempt-launch",
            "provider": {
                "exit_code": 1,
                "output": "provider dialog\nInsufficient account funds",
            },
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(!notice.starts_with('{'));
    assert!(notice.contains("Nora"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("attempt-launch"));
    assert!(notice.contains("Insufficient account funds"));
    assert!(notice.len() < 280);
}

#[test]
fn task_startup_failure_detail_is_in_the_short_worker_notice() {
    let notice = worker_lifecycle_notice(
        "Otto",
        Some("opencode"),
        Some("Insufficient account funds"),
        &json!({
            "kind": "worker_ack_startup_failed",
            "task_id": "task_g94uu5",
            "attempt_id": "attempt_rjd7ab",
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(notice.contains("Otto"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("task_g94uu5"));
    assert!(notice.contains("attempt_rjd7ab"));
    assert!(notice.contains("Insufficient account funds"));
    assert!(notice.len() < 280);
}

#[test]
fn startup_notice_to_unbound_parent_is_withheld_then_delivered_after_binding() {
    let (fixture, parent_tab_id, worker_tab_id) = lifecycle_fixture(true);
    let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();
    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        &worker_tab_id,
        "Worker",
        json!({ "kind": "agent_startup_failed", "attempt_id": "launch-a" }),
        &delivery,
    );
    assert!(delivery.writes.lock().is_empty());

    let report_id = {
        let mut store = fixture.service.report_store.lock();
        let pending = store.pending_for(&parent_tab_id);
        assert_eq!(pending.len(), 1);
        assert_eq!(store.get(&parent_tab_id, false).len(), 1);
        pending[0].id.clone()
    };
    {
        let mut mux = fixture.service.mux.write();
        let parent = mux.get_tab_mut(&parent_tab_id).unwrap();
        parent.agent_session_id = Some("parent-session".into());
        parent.session_binding_nonce = Some("parent-nonce".into());
        parent.reported_session_binding_nonce = Some("parent-nonce".into());
    }

    fixture
        .service
        .replay_pending_reports_with_delivery(&parent_tab_id, &delivery);
    assert!(fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent_tab_id)
        .is_empty());
    assert_eq!(
        *delivery.writes.lock(),
        vec![(parent_tab_id.clone(), report_id)]
    );
}

#[test]
fn startup_notice_to_a_plain_shell_parent_is_never_written() {
    let (fixture, parent_tab_id, worker_tab_id) = lifecycle_fixture(false);
    let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();
    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        &worker_tab_id,
        "Worker",
        json!({ "kind": "agent_startup_overdue", "attempt_id": "launch-b" }),
        &delivery,
    );

    let pending = fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent_tab_id);
    assert_eq!(pending.len(), 1);
    assert!(delivery.writes.lock().is_empty());
}

#[test]
fn worker_ack_notice_to_unbound_parent_remains_pending() {
    let (fixture, parent_tab_id, worker_tab_id) = lifecycle_fixture(true);
    let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();
    fixture
        .service
        .publish_worker_lifecycle_report_with_delivery(
            &parent_tab_id,
            &worker_tab_id,
            "Worker",
            "plan-a",
            "task-a",
            json!({ "kind": "worker_ack_startup_failed", "attempt_id": "attempt-a" }),
            &delivery,
        );

    let pending = fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent_tab_id);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].task_id.as_deref(), Some("task-a"));
    assert!(delivery.writes.lock().is_empty());
}
