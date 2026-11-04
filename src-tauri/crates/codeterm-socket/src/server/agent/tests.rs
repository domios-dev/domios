use super::{
    ack_log_record, startup_ack_success_response, task_ack_refusal_class, task_ack_success_response,
};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{AgentRole, TaskBinding};
use codeterm_service_core::plan::{OrchestratorRef, PlanMode, TaskStatus};
use codeterm_service_core::service::plan::NewTask;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_service_core::service::worker_ack::{WorkerAckIdentity, WorkerAckSignal};
use std::sync::Arc;

#[test]
fn agent_report_refused_kind_routes_to_diagnostic_without_recording_report() {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let worker_tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker_tab_id, AgentRole::Worker, Some(&parent_tab_id))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    let parent =
        fixture.bind_recorded_agent_recipient(&parent_tab_id, "parent-session", "parent-nonce");
    let plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "Refused report socket route".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&parent_tab_id),
            "team-refused-report".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "refused report task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker_tab_id, None, 3)
        .unwrap();
    let before = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    let service: codeterm_service_core::service::ServiceState = Arc::new(fixture.service.clone());

    let response = crate::server::dispatch_service_rpc(
        "agent.report",
        Some(serde_json::json!({
            "tab_id": worker_tab_id,
            "kind": "report_refused",
            "status": "in_review",
            "plan_id": plan.id,
            "task_id": task_id,
            "message": "REPORT NOT SENT: canonical checkout is dirty. Original report: in_review",
        })),
        &service,
    )
    .unwrap();

    assert_eq!(response["accepted"], false);
    assert_eq!(response["state"], "refused");
    assert_eq!(response["kind"], "report_refused");
    assert_eq!(response["refused_report_diagnostic_sent"], true);
    assert_eq!(response["diagnostic_target_tab_id"], parent_tab_id);
    assert_eq!(parent.texts.lock().len(), 1);
    let after = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(after.status, before.status);
    assert_eq!(
        after.execution_attempts[0].report,
        before.execution_attempts[0].report
    );
    assert_eq!(after.execution_attempts[0].report, None);
}

#[test]
fn socket_agent_ack_releases_the_armed_worker_ack_waiter() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker_tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.link_pty(&worker_tab_id, "pty-worker".into());
    {
        let worker = mux.get_tab_mut(&worker_tab_id).unwrap();
        worker.agent_session_id = Some("session-worker".into());
        worker.session_binding_nonce = Some("nonce-worker".into());
        worker.reported_session_binding_nonce = Some("nonce-worker".into());
    }
    let fixture = ServiceFixture::new(mux);
    let project = fixture.temp.path().display().to_string();
    let plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "Socket worker ACK".into(),
            project,
            OrchestratorRef::default(),
            "team-socket-ack".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut plan_state = fixture.service.plan_store.get(&plan.id).unwrap();
    plan_state.mode = PlanMode::Approved;
    fixture.service.plan_store.save(&mut plan_state, 2).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Socket ACK route".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .lane;
    let attempt_id = fixture
        .service
        .plan_store
        .begin_task_dispatch(&plan.id, &task_id, &worker_tab_id, "Ada", lane.clone(), 4)
        .unwrap();
    fixture
        .service
        .plan_store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            &worker_tab_id,
            &attempt_id,
            "session-worker",
            "nonce-worker",
            lane,
            5,
        )
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker_tab_id)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task_id.clone()),
        attempt_id: None,
        spawn_handle: None,
        bound_at: 1,
    });

    let armed = fixture.service.worker_ack.arm(WorkerAckIdentity {
        plan_id: plan.id.clone(),
        task_id: task_id.clone(),
        attempt_id: attempt_id.clone(),
        tab_id: worker_tab_id.clone(),
        startup_attempt_id: Some("pty-worker".into()),
        session_id: Some("session-worker".into()),
        binding_nonce: Some("nonce-worker".into()),
    });
    let receiver = armed.receiver.expect("worker ACK waiter armed");
    let service: codeterm_service_core::service::ServiceState = Arc::new(fixture.service.clone());

    let response = crate::server::dispatch_service_rpc(
        "agent.ack",
        Some(serde_json::json!({ "tab_id": worker_tab_id })),
        &service,
    )
    .unwrap();

    assert_eq!(response["accepted"], true);
    assert_eq!(response["state"], "in_progress");
    assert_eq!(receiver.try_recv(), Ok(WorkerAckSignal::Acknowledged));
    let task = fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert!(task.execution_attempts[0].acknowledged_at.is_some());
}

#[test]
fn repeated_task_ack_is_an_explicit_success_for_the_same_attempt() {
    let response = task_ack_success_response(
        "plan-demo",
        "task-demo",
        "worker-demo",
        "attempt-demo",
        Some(17),
        true,
        true,
    );

    assert_eq!(response["ok"], true);
    assert_eq!(response["accepted"], true);
    assert_eq!(response["already_acknowledged"], true);
    assert_eq!(response["state"], "already_acknowledged");
    assert_eq!(response["task_id"], "task-demo");
    assert_eq!(response["attempt_id"], "attempt-demo");
}

#[test]
fn first_task_ack_reports_in_progress_without_a_replay_marker() {
    let response = task_ack_success_response(
        "plan-demo",
        "task-demo",
        "worker-demo",
        "attempt-demo",
        Some(17),
        true,
        false,
    );

    assert_eq!(response["accepted"], true);
    assert_eq!(response["already_acknowledged"], false);
    assert_eq!(response["state"], "in_progress");
}

#[test]
fn repeated_startup_ack_is_an_explicit_success_for_the_attempt() {
    let response = startup_ack_success_response(
            "tab-demo",
            "attempt-demo",
            codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement::AlreadyAcknowledged,
        );

    assert_eq!(response["ok"], true);
    assert_eq!(response["accepted"], true);
    assert_eq!(response["already_acknowledged"], true);
    assert_eq!(response["state"], "already_acknowledged");
    assert_eq!(response["tab_id"], "tab-demo");
    assert_eq!(response["attempt_id"], "attempt-demo");
}

#[test]
fn startup_ack_without_an_armed_attempt_is_successfully_not_required() {
    let response = startup_ack_success_response(
        "tab-demo",
        "attempt-demo",
        codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement::NotRequired,
    );

    assert_eq!(response["ok"], true);
    assert_eq!(response["accepted"], false);
    assert_eq!(response["state"], "not_required");
}

#[test]
fn agent_ack_diagnostic_startup_accepted() {
    let record = ack_log_record(
        "startup_ack",
        "accepted",
        "none",
        Some("tab-safe"),
        Some("attempt-safe"),
    );
    assert_eq!(record["stage"], "startup_ack");
    assert_eq!(record["outcome"], "accepted");
    assert_eq!(record["attempt_id"], "attempt-safe");
}

#[test]
fn agent_ack_diagnostic_task_accepted() {
    let record = ack_log_record(
        "task_ack",
        "accepted",
        "none",
        Some("tab-safe"),
        Some("attempt-safe"),
    );
    assert_eq!(record["stage"], "task_ack");
    assert_eq!(record["outcome"], "accepted");
    assert_eq!(record["tab_id"], "tab-safe");
}

#[test]
fn agent_ack_diagnostic_refused_classified() {
    assert_eq!(
        task_ack_refusal_class("worker_ack_attempt_superseded: token=secret binding_nonce=private"),
        "worker_ack_attempt_superseded"
    );
    assert_eq!(
        task_ack_refusal_class("worker_ack_ambiguous_live_attempts: plan/task details"),
        "worker_ack_ambiguous_live_attempts"
    );
    assert_eq!(
        task_ack_refusal_class("unexpected provider key material"),
        "worker_ack_refused"
    );
}

#[test]
fn agent_ack_diagnostic_stale_unattributed() {
    let record = ack_log_record("task_ack", "refused", "worker_ack_refused", None, None);
    assert_eq!(record["tab_id"], serde_json::Value::Null);
    assert_eq!(record["attempt_id"], serde_json::Value::Null);
}

#[test]
fn agent_ack_diagnostic_unauthenticated_unattributed() {
    let record = ack_log_record(
        "startup_ack",
        "refused",
        "startup_ack_not_owner",
        None,
        None,
    );
    assert_eq!(record["tab_id"], serde_json::Value::Null);
    assert_eq!(record["attempt_id"], serde_json::Value::Null);
    assert_eq!(record["refusal_class"], "startup_ack_not_owner");
}

#[test]
fn agent_ack_diagnostic_redacts_secrets() {
    let raw_error =
        "worker_ack_attempt_superseded: token=secret binding_nonce=private raw_error=hidden";
    let record = ack_log_record(
        "task_ack",
        "refused",
        task_ack_refusal_class(raw_error),
        None,
        None,
    );
    assert!(!record.to_string().contains("binding_nonce"));
    assert!(!record.to_string().contains("token"));
    assert!(!record.to_string().contains("raw_error"));
    assert!(!record.to_string().contains("secret"));
    assert!(!record.to_string().contains("private"));
}
