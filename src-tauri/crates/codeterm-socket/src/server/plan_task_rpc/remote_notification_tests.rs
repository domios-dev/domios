use super::remote_lifecycle::{remote_ack, remote_report};
use codeterm_service_core::agent::reports::ReportStatus;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::{OrchestratorRef, Plan, TaskStatus};
use codeterm_service_core::service::plan::NewTask;
use codeterm_service_core::service::test_fixture::{RecordedAgentDeliveries, ServiceFixture};
use codeterm_service_core::service::worker_ack::remote_deadline::remote_worker_ack_identity;
use codeterm_service_core::service::worker_ack::WorkerAckSignal;
use serde_json::json;
use std::sync::Arc;

struct RemoteFixture {
    fixture: ServiceFixture,
    service: codeterm_service_core::service::ServiceState,
    orchestrator: String,
    deliveries: RecordedAgentDeliveries,
    plan_id: String,
    task_id: String,
    attempt_id: String,
    peer_id: String,
    worker_tab_id: String,
}

fn remote_fixture() -> RemoteFixture {
    let (mut mux, orchestrator) = MultiplexerState::new();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let deliveries =
        fixture.bind_recorded_agent_recipient(&orchestrator, "orch-session", "orch-nonce");
    let mut plan = Plan::new(
        "Remote delivery".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&orchestrator),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Remote notification".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["origin orchestrator receives ACK and report".into()],
                test_plan: Some("focused socket test".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = "attempt_remote_notify".to_string();
    let peer_id = "peer-acceptance-r161".to_string();
    let worker_tab_id = "remote-worker-tab".to_string();
    fixture
        .service
        .plan_store
        .begin_remote_dispatch_attempt(
            &plan.id,
            &task_id,
            &attempt_id,
            &peer_id,
            "acceptance-r161",
            "opaque-provider",
            None,
            Some(&orchestrator),
            "base-sha",
            "integ/1.23.3",
            3,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .record_remote_dispatch_lane(
            &plan.id,
            &task_id,
            &attempt_id,
            &peer_id,
            &worker_tab_id,
            None,
            "lane/remote-worker",
            "D:/peer/lane",
            "base-sha",
            "opaque-provider",
            "worker-nonce",
            4,
        )
        .unwrap();
    let service = Arc::new(fixture.service.clone());
    RemoteFixture {
        fixture,
        service,
        orchestrator,
        deliveries,
        plan_id: plan.id,
        task_id,
        attempt_id,
        peer_id,
        worker_tab_id,
    }
}

fn ui_agent_reports(fixture: &ServiceFixture) -> Vec<serde_json::Value> {
    fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(event, _)| event == "agent-report")
        .map(|(_, payload)| payload.clone())
        .collect()
}

fn bus_agent_reports(
    events: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    while let Ok(envelope) = events.try_recv() {
        if let codeterm_events::SystemEvent::Generic { event_type, data } = envelope.event {
            if event_type == "agent.report" {
                found.push(data);
            }
        }
    }
    found
}

#[test]
fn remote_ack_notifies_owning_orchestrator_once_and_ignores_replay() {
    let rf = remote_fixture();
    let armed = rf
        .fixture
        .service
        .worker_ack
        .arm(remote_worker_ack_identity(
            &rf.plan_id,
            &rf.task_id,
            &rf.attempt_id,
            &rf.peer_id,
            &rf.worker_tab_id,
        ));
    let receiver = armed.receiver.expect("remote ACK waiter armed");
    let mut bus = rf.fixture.service.events.subscribe();
    let params = Some(json!({
        "plan_id": rf.plan_id,
        "task_id": rf.task_id,
        "attempt_id": rf.attempt_id,
        "worker_tab_id": rf.worker_tab_id,
        "authenticated_mesh_peer_id": rf.peer_id,
    }));

    let first = remote_ack(&params, &rf.service, 100).unwrap();
    assert_eq!(first["acknowledged"], true);
    assert_eq!(receiver.try_recv(), Ok(WorkerAckSignal::Acknowledged));

    let task = rf
        .fixture
        .service
        .plan_store
        .inspect_task_for_test(&rf.task_id)
        .unwrap()
        .task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.execution_attempts[0].acknowledged_at, Some(100));

    let delivered = rf.deliveries.texts.lock().clone();
    assert_eq!(delivered.len(), 1);
    assert!(
        delivered[0].contains("acknowledged task"),
        "{}",
        delivered[0]
    );
    assert!(delivered[0].contains(&rf.task_id), "{}", delivered[0]);
    assert!(delivered[0].contains(&rf.attempt_id), "{}", delivered[0]);

    let stored = rf
        .fixture
        .service
        .report_store
        .lock()
        .get(&rf.orchestrator, false);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].kind.as_deref(), Some("worker_ack"));
    assert_eq!(stored[0].from_tab_id, rf.worker_tab_id);
    assert_eq!(stored[0].from_title, "Worker@acceptance-r161");
    assert_eq!(stored[0].plan_id.as_deref(), Some(rf.plan_id.as_str()));
    assert_eq!(stored[0].task_id.as_deref(), Some(rf.task_id.as_str()));
    assert!(rf
        .fixture
        .service
        .report_store
        .lock()
        .pending_for(&rf.orchestrator)
        .is_empty());

    let ui_events = ui_agent_reports(&rf.fixture);
    assert_eq!(ui_events.len(), 1);
    assert_eq!(ui_events[0]["target_tab_id"], rf.orchestrator);
    assert_eq!(ui_events[0]["kind"], "worker_ack");

    let bus_events = bus_agent_reports(&mut bus);
    assert_eq!(bus_events.len(), 1);
    assert_eq!(bus_events[0]["target_tab_id"], rf.orchestrator);
    assert_eq!(bus_events[0]["kind"], "worker_ack");

    let replay = remote_ack(&params, &rf.service, 101).unwrap();
    assert_eq!(replay["acknowledged"], false);
    assert_eq!(rf.deliveries.texts.lock().len(), 1);
    assert_eq!(
        rf.fixture
            .service
            .report_store
            .lock()
            .get(&rf.orchestrator, false)
            .len(),
        1
    );
    assert_eq!(ui_agent_reports(&rf.fixture).len(), 1);
    assert!(bus_agent_reports(&mut bus).is_empty());
}

#[test]
fn remote_report_delivers_to_owning_orchestrator_once_and_marks_target_written() {
    let rf = remote_fixture();
    rf.fixture
        .service
        .plan_store
        .acknowledge_remote_dispatch(
            &rf.plan_id,
            &rf.task_id,
            &rf.attempt_id,
            &rf.peer_id,
            &rf.worker_tab_id,
            50,
        )
        .unwrap();
    let params = Some(json!({
        "plan_id": rf.plan_id,
        "task_id": rf.task_id,
        "attempt_id": rf.attempt_id,
        "worker_tab_id": rf.worker_tab_id,
        "authenticated_mesh_peer_id": rf.peer_id,
        "report": {
            "id": format!("remote-report:{}", rf.attempt_id),
            "message": "Implemented remote report delivery",
            "status": "in_review",
            "reporter_tab_id": rf.worker_tab_id,
            "sha": "deadbeef",
            "changed_files": ["src-tauri/crates/codeterm-socket/src/server/plan_task_rpc.rs"],
            "test_cmd": "cargo test",
            "test_result": "pass",
            "skipped": "live peer"
        }
    }));

    let first = remote_report(&params, &rf.service, 200).unwrap();
    assert_eq!(first["reported"], true);

    let task = rf
        .fixture
        .service
        .plan_store
        .inspect_task_for_test(&rf.task_id)
        .unwrap()
        .task;
    assert_eq!(task.status, TaskStatus::InReview);
    assert!(
        task.execution_attempts[0]
            .report_target_written_at
            .is_some(),
        "delivering a remote in_review report must record report_target_written_at"
    );

    let delivered = rf.deliveries.texts.lock().clone();
    assert_eq!(delivered.len(), 1);
    assert!(
        delivered[0].contains(&format!("task: {}/{}", rf.plan_id, rf.task_id)),
        "{}",
        delivered[0]
    );
    assert!(
        delivered[0].contains("status: In Review"),
        "{}",
        delivered[0]
    );
    assert!(
        delivered[0].contains("branch: lane/remote-worker"),
        "{}",
        delivered[0]
    );
    assert!(delivered[0].contains("sha: deadbeef"), "{}", delivered[0]);
    assert!(
        delivered[0].contains("tests: cargo test -> pass"),
        "{}",
        delivered[0]
    );
    assert!(
        delivered[0].contains("skipped: live peer"),
        "{}",
        delivered[0]
    );
    assert!(
        delivered[0].contains("Implemented remote report delivery"),
        "{}",
        delivered[0]
    );
    assert!(
        delivered[0].contains(&format!(
            "Next: domios plan review accept|bounce {} {}",
            rf.plan_id, rf.task_id
        )),
        "{}",
        delivered[0]
    );

    let stored = rf
        .fixture
        .service
        .report_store
        .lock()
        .get(&rf.orchestrator, false);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].from_tab_id, rf.worker_tab_id);
    assert_eq!(stored[0].from_title, "Worker@acceptance-r161");
    assert_eq!(stored[0].plan_id.as_deref(), Some(rf.plan_id.as_str()));
    assert_eq!(stored[0].task_id.as_deref(), Some(rf.task_id.as_str()));
    assert_eq!(
        stored[0].attempt_id.as_deref(),
        Some(rf.attempt_id.as_str())
    );
    assert_eq!(stored[0].status, Some(ReportStatus::InReview));
    assert_eq!(stored[0].branch.as_deref(), Some("lane/remote-worker"));
    assert_eq!(stored[0].sha.as_deref(), Some("deadbeef"));
    assert!(rf
        .fixture
        .service
        .report_store
        .lock()
        .pending_for(&rf.orchestrator)
        .is_empty());

    let ui_events = ui_agent_reports(&rf.fixture);
    assert_eq!(ui_events.len(), 1);
    assert_eq!(ui_events[0]["target_tab_id"], rf.orchestrator);
    assert_eq!(ui_events[0]["from_tab_id"], rf.worker_tab_id);
    assert_eq!(ui_events[0]["task_report"]["persisted"], true);
    assert_eq!(ui_events[0]["task_report"]["plan_id"], rf.plan_id);
    assert_eq!(ui_events[0]["task_report"]["task_id"], rf.task_id);

    let replay = remote_report(&params, &rf.service, 201).unwrap();
    assert_eq!(replay["reported"], false);
    assert_eq!(rf.deliveries.texts.lock().len(), 1);
    assert_eq!(
        rf.fixture
            .service
            .report_store
            .lock()
            .get(&rf.orchestrator, false)
            .len(),
        1
    );
    assert_eq!(ui_agent_reports(&rf.fixture).len(), 1);
}

#[test]
fn remote_ack_and_report_follow_current_plan_owner_after_handoff() {
    let rf = remote_fixture();
    let next_orch = {
        let mut mux = rf.fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&rf.orchestrator).unwrap();
        let tab = mux.create_tab(&workspace).unwrap();
        mux.set_orchestrator_role(&tab).unwrap();
        tab
    };
    let next_deliveries = rf.fixture.bind_recorded_agent_recipient(
        &next_orch,
        "next-orch-session",
        "next-orch-nonce",
    );
    let mut plan = rf.fixture.service.plan_store.get(&rf.plan_id).unwrap();
    plan.owner = OrchestratorRef::from_tab(&next_orch);
    rf.fixture.service.plan_store.save(&mut plan, 10).unwrap();

    let ack_params = Some(json!({
        "plan_id": rf.plan_id,
        "task_id": rf.task_id,
        "attempt_id": rf.attempt_id,
        "worker_tab_id": rf.worker_tab_id,
        "authenticated_mesh_peer_id": rf.peer_id,
    }));
    assert_eq!(
        remote_ack(&ack_params, &rf.service, 100).unwrap()["acknowledged"],
        true
    );

    let report_params = Some(json!({
        "plan_id": rf.plan_id,
        "task_id": rf.task_id,
        "attempt_id": rf.attempt_id,
        "worker_tab_id": rf.worker_tab_id,
        "authenticated_mesh_peer_id": rf.peer_id,
        "report": {
            "id": format!("remote-report:{}", rf.attempt_id),
            "message": "Completed after orchestrator handoff",
            "status": "in_review",
            "reporter_tab_id": rf.worker_tab_id,
            "sha": "cafebabe"
        }
    }));
    assert_eq!(
        remote_report(&report_params, &rf.service, 200).unwrap()["reported"],
        true
    );

    assert!(rf.deliveries.texts.lock().is_empty());
    let next_texts = next_deliveries.texts.lock().clone();
    assert_eq!(next_texts.len(), 2);
    assert!(
        next_texts[0].contains("acknowledged task"),
        "{}",
        next_texts[0]
    );
    assert!(
        next_texts[1].contains("Completed after orchestrator handoff"),
        "{}",
        next_texts[1]
    );
}

#[test]
fn remote_report_validation_refusal_delivers_nothing() {
    let rf = remote_fixture();
    rf.fixture
        .service
        .plan_store
        .acknowledge_remote_dispatch(
            &rf.plan_id,
            &rf.task_id,
            &rf.attempt_id,
            &rf.peer_id,
            &rf.worker_tab_id,
            50,
        )
        .unwrap();

    for report in [
        json!({
            "id": "",
            "message": "missing id",
            "status": "in_review",
            "reporter_tab_id": rf.worker_tab_id,
        }),
        json!({
            "id": "remote-report-bad-status",
            "message": "wrong status",
            "status": "done",
            "reporter_tab_id": rf.worker_tab_id,
        }),
    ] {
        let params = Some(json!({
            "plan_id": rf.plan_id,
            "task_id": rf.task_id,
            "attempt_id": rf.attempt_id,
            "worker_tab_id": rf.worker_tab_id,
            "authenticated_mesh_peer_id": rf.peer_id,
            "report": report,
        }));
        let err = remote_report(&params, &rf.service, 200).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    assert!(rf.deliveries.texts.lock().is_empty());
    assert!(rf
        .fixture
        .service
        .report_store
        .lock()
        .get(&rf.orchestrator, false)
        .is_empty());
    assert!(ui_agent_reports(&rf.fixture).is_empty());
}
