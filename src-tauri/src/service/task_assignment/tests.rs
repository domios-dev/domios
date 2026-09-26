use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service::plan::task_attempts::BoundWorkerReport;
use crate::service::plan::NewTask;
use codeterm_types::agent::ReportStatus;

#[allow(clippy::too_many_arguments)]
fn assign_existing_task(
    store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    peer: Option<&str>,
    authority: Option<&TaskMutationAuthority>,
    now: u64,
) -> Result<(), PlanError> {
    let assignment = prepare_existing_task_assignment(mux, tab_id, peer, authority)?;
    persist_existing_task_assignment(
        store, assignment, plan_id, task_id, tab_id, peer, authority, now,
    )
}

fn fixture() -> (
    tempfile::TempDir,
    PlanStore,
    MultiplexerState,
    String,
    String,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let (mut mux, tab) = MultiplexerState::new();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.agent_session_id = Some("verified-session".into());
    target.session_binding_nonce = Some("test-nonce".into());
    target.reported_session_binding_nonce = Some("test-nonce".into());
    let mut plan = Plan::new(
        "assignment".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&tab),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "lifecycle".into(),
                acceptance_criteria: vec!["exact bound continuation".into()],
                test_plan: Some("focused lifecycle check".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, mux, plan.id, task, tab)
}

fn roundtrip(authorized: bool) {
    let (_dir, store, mux, plan, task, tab) = fixture();
    let owner = TaskMutationAuthority {
        tab_id: tab.clone(),
        session_id: "verified-session".into(),
        session_binding_nonce: "test-nonce".into(),
        owner_epoch: 0,
    };
    assign_existing_task(
        &store,
        &mux,
        &plan,
        &task,
        &tab,
        None,
        authorized.then_some(&owner),
        3,
    )
    .unwrap();
    let assigned = store.show_task(&task).unwrap().task;
    let attempt = &assigned.execution_attempts[0];
    assert_eq!(attempt.session_id.as_deref(), Some("verified-session"));
    assert!(attempt.acknowledged_at.is_none());
    store
        .acknowledge_task_attempt(&plan, &task, &tab, &attempt.id, 4)
        .unwrap();
    let authority = TaskMutationAuthority {
        owner_epoch: assigned.interruption.owner_epoch,
        ..owner
    };
    assert!(store
        .ingest_bound_worker_report(
            &plan,
            &task,
            &AgentRef::from_tab(&tab),
            BoundWorkerReport {
                message: "candidate".into(),
                status: Some(ReportStatus::InReview),
                sha: Some("abcdef123456".into()),
                authority: Some(authority),
                ..Default::default()
            },
            5
        )
        .unwrap());
    let bounce = store
        .bounce_review(
            &plan,
            &task,
            "repair same oracle",
            false,
            false,
            AgentRef::from_tab(&tab),
            6,
        )
        .unwrap();
    let continuation = bounce.continuation.unwrap();
    assert_eq!(continuation.attempt_id, attempt.id);
    assert_eq!(continuation.session_id, "verified-session");
    store
        .claim_review_continuation_delivery(&plan, &task, &attempt.id, continuation.epoch, 7)
        .expect("valid continuation must be claimed");
    let claimed = store.show_task(&task).unwrap().task;
    assert_eq!(claimed.status, TaskStatus::InReview);
    let claimed_attempt = claimed
        .execution_attempts
        .iter()
        .find(|current| current.id == attempt.id)
        .expect("same attempt remains persisted");
    assert_eq!(
        claimed_attempt.session_id.as_deref(),
        Some("verified-session")
    );
    assert_eq!(claimed_attempt.continuation.epoch, continuation.epoch);
    assert_eq!(
        claimed_attempt.continuation.state.as_deref(),
        Some("continuation_delivering")
    );
    assert_eq!(claimed_attempt.acknowledged_at, Some(4));
    assert_eq!(claimed_attempt.reported_at, Some(5));
    let before_refusal = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    let refusal = store
        .claim_review_continuation_delivery(&plan, &task, &attempt.id, continuation.epoch, 7)
        .expect_err("a claimed continuation cannot be claimed twice");
    assert!(matches!(
        refusal,
        PlanError::ReviewContinuationDeliveryRefused { .. }
    ));
    assert_eq!(
        serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
        before_refusal
    );
    assert!(store
        .finish_review_continuation(&plan, &task, &attempt.id, continuation.epoch, true, 8)
        .unwrap());
    assert_eq!(
        store.show_task(&task).unwrap().task.status,
        TaskStatus::InProgress
    );
}

#[test]
fn assignment_captures_model_already_detected_on_its_bound_tab() {
    let (_dir, store, mut mux, plan, task, tab) = fixture();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.agent_type = Some("codex".into());
    target.agent_provider_resolved_model = Some("gpt-6-luna".into());
    target.agent_session_model = Some("opus".into());

    assign_existing_task(&store, &mux, &plan, &task, &tab, None, None, 3).unwrap();

    let mut task = store.show_task(&task).unwrap().task;
    let attempt = task.execution_attempts.remove(0);
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.agent_type.as_deref(), Some("codex"));
    assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(attempt.session_model.as_deref(), Some("opus"));
    assert_eq!(attempt.session_id.as_deref(), Some("verified-session"));
    assert_eq!(attempt.session_binding_nonce.as_deref(), Some("test-nonce"));
}

#[test]
fn lifecycle_reassignment_local_assignment_ack_report_bounce() {
    roundtrip(false);
}

#[test]
fn lifecycle_reassignment_authorized_binding_ack_report_bounce() {
    roundtrip(true);
}

#[test]
fn lifecycle_reassignment_missing_or_mismatched_binding_refuses_before_write() {
    let (_dir, store, mut mux, plan, task, tab) = fixture();
    let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    for (session, nonce, reported) in [
        (None, Some("test-nonce"), Some("test-nonce")),
        (Some("verified-session"), None, None),
        (Some("verified-session"), Some("test-nonce"), None),
        (
            Some("verified-session"),
            Some("test-nonce"),
            Some("foreign-nonce"),
        ),
        (Some(""), Some("test-nonce"), Some("test-nonce")),
    ] {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_session_id = session.map(str::to_string);
        target.session_binding_nonce = nonce.map(str::to_string);
        target.reported_session_binding_nonce = reported.map(str::to_string);
        assert!(assign_existing_task(&store, &mux, &plan, &task, &tab, None, None, 3).is_err());
        assert_eq!(
            serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
            before
        );
    }
    assert!(
        assign_existing_task(&store, &mux, &plan, &task, "missing-tab", None, None, 3).is_err()
    );
    assert_eq!(
        serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
        before
    );
}

#[test]
fn lifecycle_reassignment_rebound_caller_refuses_before_write() {
    let (_dir, store, mux, plan, task, tab) = fixture();
    let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    for (session, nonce) in [
        ("foreign-session", "test-nonce"),
        ("verified-session", "foreign-nonce"),
    ] {
        let caller = TaskMutationAuthority {
            tab_id: tab.clone(),
            session_id: session.into(),
            session_binding_nonce: nonce.into(),
            owner_epoch: 0,
        };
        assert!(
            assign_existing_task(&store, &mux, &plan, &task, &tab, None, Some(&caller), 3).is_err()
        );
        assert_eq!(
            serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn lifecycle_reassignment_remote_never_copies_colliding_local_identity() {
    let (_dir, store, mux, plan, task, tab) = fixture();
    let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
    assert!(assign_existing_task(&store, &mux, &plan, &task, &tab, Some(" "), None, 3).is_err());
    assert_eq!(
        serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
        before
    );
    assign_existing_task(
        &store,
        &mux,
        &plan,
        &task,
        &tab,
        Some("remote-peer"),
        None,
        3,
    )
    .unwrap();
    let assigned = store.show_task(&task).unwrap().task;
    let attempt = &assigned.execution_attempts[0];
    assert_eq!(attempt.mesh_peer.as_deref(), Some("remote-peer"));
    assert!(attempt.session_id.is_none());
    assert!(attempt.session_binding_nonce.is_none());
    assert!(attempt.acknowledged_at.is_none());
}

#[test]
fn lifecycle_reassignment_local_assignment_preserves_active_and_held_review() {
    let (_dir, store, mux, plan, task, tab) = fixture();
    assign_existing_task(&store, &mux, &plan, &task, &tab, None, None, 3).unwrap();
    let initial = store.show_task(&task).unwrap().task;
    store
        .acknowledge_task_attempt(&plan, &task, &tab, &initial.execution_attempts[0].id, 4)
        .unwrap();
    for in_review in [false, true] {
        if in_review {
            let authority = TaskMutationAuthority {
                tab_id: tab.clone(),
                session_id: "verified-session".into(),
                session_binding_nonce: "test-nonce".into(),
                owner_epoch: initial.interruption.owner_epoch,
            };
            store
                .ingest_bound_worker_report(
                    &plan,
                    &task,
                    &AgentRef::from_tab(&tab),
                    BoundWorkerReport {
                        message: "candidate".into(),
                        status: Some(ReportStatus::InReview),
                        sha: Some("abcdef123456".into()),
                        authority: Some(authority),
                        ..Default::default()
                    },
                    5,
                )
                .unwrap();
            store
                .hold_review(&plan, &task, "held candidate", AgentRef::from_tab(&tab), 6)
                .unwrap();
        }
        let before = serde_json::to_value(store.get(&plan).unwrap()).unwrap();
        assert!(assign_existing_task(&store, &mux, &plan, &task, &tab, None, None, 7).is_err());
        assert_eq!(
            serde_json::to_value(store.get(&plan).unwrap()).unwrap(),
            before
        );
    }
}
