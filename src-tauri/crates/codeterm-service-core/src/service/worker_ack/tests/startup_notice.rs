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
        "domios agent startup-hold --tab child --attempt launch-a"
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
    assert!(notice.contains("acknowledged task task_g94uu5"));
    assert!(notice.len() < 180);
}

#[test]
fn dispatched_chat_backend_readiness_notice_names_ready_state_and_task() {
    let notice = worker_lifecycle_notice(
        "Router",
        Some("fixture-agent"),
        None,
        &json!({
            "kind": "dispatch_ready",
            "plan_id": "pl_7lsisa",
            "task_id": "task_cjymmf",
            "attempt_id": "attempt_svwudi",
        }),
    );

    assert_eq!(
        notice,
        "Agent Router (fixture-agent) is ready for task task_cjymmf, attempt attempt_svwudi."
    );
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

#[test]
fn lifecycle_provider_read_failure_precedes_report_persistence_and_delivery() {
    let (fixture, parent, worker) = lifecycle_fixture(true);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker)
        .unwrap()
        .agent_type = None;
    let mut plan = crate::plan::Plan::new(
        "opaque".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab(&parent),
    );
    let mut task: crate::plan::Task = serde_json::from_value(
        json!({"id":"lifecycle-task","name":"opaque","type":"code","status":"pending"}),
    )
    .unwrap();
    task.execution_attempts
        .push(crate::plan::TaskExecutionAttempt {
            id: "lifecycle-attempt".into(),
            executor_tab_id: worker.clone(),
            provider: Some("opaque-provider".into()),
            ..Default::default()
        });
    plan.tasks.push(task);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let connection =
        rusqlite::Connection::open(fixture.temp.path().join(".domios/plans.db")).unwrap();
    connection.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.provider',42) WHERE task_id='lifecycle-task'",[]).unwrap();
    let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();
    fixture
        .service
        .publish_worker_lifecycle_report_with_delivery(
            &parent,
            &worker,
            "Worker",
            &plan.id,
            "lifecycle-task",
            json!({"kind":"worker_ack_startup_failed","attempt_id":"lifecycle-attempt"}),
            &delivery,
        );
    assert!(fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent)
        .is_empty());
    assert!(delivery.writes.lock().is_empty());
    connection.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.provider','opaque-provider') WHERE task_id='lifecycle-task'",[]).unwrap();
    fixture
        .service
        .publish_worker_lifecycle_report_with_delivery(
            &parent,
            &worker,
            "Worker",
            &plan.id,
            "lifecycle-task",
            json!({"kind":"worker_ack_startup_failed","attempt_id":"lifecycle-attempt"}),
            &delivery,
        );
    assert_eq!(
        fixture
            .service
            .report_store
            .lock()
            .pending_for(&parent)
            .len(),
        1
    );
}

fn next_bus_agent_report(
    events: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
) -> Value {
    while let Ok(envelope) = events.try_recv() {
        if let codeterm_events::SystemEvent::Generic { event_type, data } = envelope.event {
            if event_type == "agent.report" {
                return data;
            }
        }
    }
    panic!("expected agent.report bus event");
}

fn last_ui_agent_report(fixture: &crate::service::test_fixture::ServiceFixture) -> Value {
    fixture
        .ui
        .0
        .lock()
        .iter()
        .rev()
        .find(|(event, _)| event == "agent-report")
        .map(|(_, payload)| payload.clone())
        .expect("expected agent-report UI event")
}

#[test]
fn startup_notices_capture_bound_sender_position_before_later_rows_and_tool_patches() {
    for (kind, via_worker_lifecycle) in [("dispatch_ready", false), ("worker_ack", true)] {
        let (fixture, parent_tab_id, worker_tab_id) = lifecycle_fixture(true);
        let worker_tab = {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&worker_tab_id).unwrap();
            tab.agent_type = Some("acp".into());
            tab.cwd = Some("/workspace".into());
            tab.agent_session_id = Some("worker-session".into());
            tab.session_binding_nonce = Some("worker-nonce".into());
            tab.clone()
        };
        let _runtime =
            crate::service::agent_runtime::engine::test_support::live_runtime(&worker_tab);
        let session = crate::service::agent_runtime::engine::for_tab(&worker_tab_id).unwrap();
        session
            .cache
            .lock()
            .reconcile(crate::agent::chat_types::ChatMsg {
                id: "ack-row".into(),
                kind: "assistant".into(),
                content: "acknowledging".into(),
                ..Default::default()
            });
        let mut bus = fixture.service.events.subscribe();
        let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();

        if via_worker_lifecycle {
            fixture
                .service
                .publish_worker_lifecycle_report_with_delivery(
                    &parent_tab_id,
                    &worker_tab_id,
                    "Order",
                    "pl_fixture",
                    "task_fixture",
                    json!({ "kind": kind, "attempt_id": "attempt_l33cqd" }),
                    &delivery,
                );
        } else {
            fixture.service.publish_agent_startup_report_with_delivery(
                &parent_tab_id,
                &worker_tab_id,
                "Order",
                json!({
                    "kind": kind,
                    "plan_id": "pl_fixture",
                    "task_id": "task_fixture",
                    "attempt_id": "attempt_l33cqd",
                }),
                &delivery,
            );
        }

        session
            .cache
            .lock()
            .reconcile(crate::agent::chat_types::ChatMsg {
                id: "later-assistant".into(),
                kind: "assistant".into(),
                content: "working".into(),
                ..Default::default()
            });
        session
            .cache
            .lock()
            .reconcile(crate::agent::chat_types::ChatMsg {
                id: "later-tool".into(),
                kind: "tool_call".into(),
                content: "running".into(),
                ..Default::default()
            });
        session
            .cache
            .lock()
            .reconcile(crate::agent::chat_types::ChatMsg {
                id: "later-tool".into(),
                kind: "tool_call".into(),
                content: "patched".into(),
                ..Default::default()
            });

        let expected_position =
            json!({ "source_session_id": "worker-session", "message_id": "ack-row" });
        let bus_event = next_bus_agent_report(&mut bus);
        let ui_event = last_ui_agent_report(&fixture);
        assert_eq!(bus_event["sender_position"], expected_position);
        assert_eq!(ui_event["sender_position"], expected_position);
        assert_eq!(bus_event["kind"], json!(kind));
        assert_eq!(ui_event["kind"], json!(kind));
        assert_eq!(bus_event["report"]["id"], ui_event["report"]["id"]);
        assert!(ui_event["report"]["status"].is_null());
        assert_eq!(bus_event["report"]["sender_position"], expected_position);
        assert_eq!(bus_event["report"]["kind"], json!(kind));
        let stored = fixture
            .service
            .report_store
            .lock()
            .get(&parent_tab_id, false);
        let stored_report = stored
            .iter()
            .find(|report| report.id == bus_event["report"]["id"])
            .expect("startup report is stored before publication");
        assert_eq!(
            stored_report.sender_position.as_ref().unwrap().message_id,
            "ack-row"
        );
        assert_eq!(stored_report.kind.as_deref(), Some(kind));
        assert_eq!(
            crate::service::agent_runtime::report_position::sender_position(
                &fixture.service,
                &worker_tab_id,
            ),
            Some(crate::agent::reports::ReportSenderPosition {
                source_session_id: "worker-session".into(),
                message_id: "later-tool".into(),
            })
        );
    }
}

#[test]
fn startup_notices_keep_null_sender_position_on_empty_cache_or_replaced_binding() {
    let (fixture, parent_tab_id, worker_tab_id) = lifecycle_fixture(true);
    let worker_tab = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&worker_tab_id).unwrap();
        tab.agent_type = Some("acp".into());
        tab.cwd = Some("/workspace".into());
        tab.agent_session_id = Some("worker-session".into());
        tab.session_binding_nonce = Some("worker-nonce".into());
        tab.clone()
    };
    let runtime = crate::service::agent_runtime::engine::test_support::live_runtime(&worker_tab);
    let session = crate::service::agent_runtime::engine::for_tab(&worker_tab_id).unwrap();
    let mut bus = fixture.service.events.subscribe();
    let delivery = crate::service::test_fixture::RecordingHierarchyReportDelivery::default();

    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        &worker_tab_id,
        "Order",
        json!({ "kind": "dispatch_ready", "task_id": "task_fixture", "attempt_id": "attempt_empty" }),
        &delivery,
    );
    assert_eq!(
        next_bus_agent_report(&mut bus)["sender_position"],
        Value::Null
    );
    assert_eq!(
        last_ui_agent_report(&fixture)["sender_position"],
        Value::Null
    );

    session
        .cache
        .lock()
        .reconcile(crate::agent::chat_types::ChatMsg {
            id: "seed-row".into(),
            kind: "assistant".into(),
            content: "seeded".into(),
            ..Default::default()
        });

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker_tab_id)
        .unwrap()
        .session_binding_nonce = Some("replaced-nonce".into());
    fixture
        .service
        .publish_worker_lifecycle_report_with_delivery(
            &parent_tab_id,
            &worker_tab_id,
            "Order",
            "pl_fixture",
            "task_fixture",
            json!({ "kind": "worker_ack", "attempt_id": "attempt_nonce" }),
            &delivery,
        );
    assert_eq!(
        next_bus_agent_report(&mut bus)["sender_position"],
        Value::Null
    );
    assert_eq!(
        last_ui_agent_report(&fixture)["sender_position"],
        Value::Null
    );

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&worker_tab_id).unwrap();
        tab.session_binding_nonce = Some("worker-nonce".into());
        tab.agent_session_id = Some("replaced-session".into());
    }
    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        &worker_tab_id,
        "Order",
        json!({ "kind": "dispatch_ready", "task_id": "task_fixture", "attempt_id": "attempt_session" }),
        &delivery,
    );
    assert_eq!(
        next_bus_agent_report(&mut bus)["sender_position"],
        Value::Null
    );
    assert_eq!(
        last_ui_agent_report(&fixture)["sender_position"],
        Value::Null
    );

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker_tab_id)
        .unwrap()
        .agent_session_id = Some("worker-session".into());
    drop(runtime);
    fixture
        .service
        .publish_worker_lifecycle_report_with_delivery(
            &parent_tab_id,
            &worker_tab_id,
            "Order",
            "pl_fixture",
            "task_fixture",
            json!({ "kind": "worker_ack", "attempt_id": "attempt_stopped" }),
            &delivery,
        );
    assert_eq!(
        next_bus_agent_report(&mut bus)["sender_position"],
        Value::Null
    );
    assert_eq!(
        last_ui_agent_report(&fixture)["sender_position"],
        Value::Null
    );
}
