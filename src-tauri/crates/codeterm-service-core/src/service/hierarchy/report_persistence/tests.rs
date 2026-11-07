use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn missing_report_identity_resolves_only_from_exact_live_tab_binding() {
    let details = ReportDetails {
        caller_tab_id: Some("worker".into()),
        ..Default::default()
    };
    assert_eq!(
        resolve_missing_report_identity(
            &details,
            "worker",
            Some("live-session"),
            Some("live-nonce"),
            Some("live-nonce"),
        ),
        Some(("live-session".into(), "live-nonce".into()))
    );
    assert!(resolve_missing_report_identity(
        &details,
        "other-tab",
        Some("live-session"),
        Some("live-nonce"),
        Some("live-nonce"),
    )
    .is_none());
    assert!(resolve_missing_report_identity(
        &details,
        "worker",
        Some("live-session"),
        Some("live-nonce"),
        Some("unreported-nonce"),
    )
    .is_none());
    let stale_claim = ReportDetails {
        session_binding_nonce: Some("stale-nonce".into()),
        ..details
    };
    assert!(resolve_missing_report_identity(
        &stale_claim,
        "worker",
        Some("live-session"),
        Some("live-nonce"),
        Some("live-nonce"),
    )
    .is_none());
}

#[test]
fn missing_review_evidence_has_its_own_report_outcome() {
    let failure = plan_failure(
        "worker",
        "plan",
        "task",
        crate::service::plan::PlanError::ReviewEvidenceRequired,
    );

    assert_eq!(
        failure.report_outcome,
        ReportOutcomeClass::MissingReviewEvidence
    );
}

#[test]
fn rejected_shared_commit_reaches_the_parent_as_rejected_with_the_worker_next_step() {
    let cause = "reported commit 152cefbf is not on the shared checkout HEAD 57521bf1; it may have been reset or amended";
    let failure = plan_failure(
        "worker",
        "plan",
        "task",
        crate::service::plan::PlanError::SharedReportRejected(cause.into()),
    );
    assert_eq!(
        failure.report_outcome,
        ReportOutcomeClass::TransitionRejected
    );
    assert!(failure.cause.starts_with(cause), "{}", failure.cause);
    assert!(failure.cause.contains(SHARED_REPORT_NEXT_STEP));
    assert!(!failure.cause.contains("PRODUCT INCIDENT"));
    assert!(!failure.cause.contains("--plan and --task"));

    let delivered =
        rejected_report_delivery_message(failure.report_outcome, &failure.cause, "evidence");
    assert!(delivered.starts_with("REPORT RECEIVED — TASK TRANSITION REJECTED\n"));
    assert!(delivered.contains(&format!("Cause: {cause}")));
    assert!(delivered.contains("do not accept or bounce it"));
    assert!(!delivered.contains("PRODUCT INCIDENT"));
    assert!(!delivered.contains("Owner action required"));
    assert!(delivered.ends_with("\n\nevidence"));
}

#[test]
fn a_real_persistence_incident_stays_labelled_as_an_incident() {
    let failure = plan_failure(
        "worker",
        "plan",
        "task",
        crate::service::plan::PlanError::Io("database is locked".into()),
    );
    assert_eq!(
        failure.report_outcome,
        ReportOutcomeClass::StaleBindingConflict
    );
    assert!(failure.cause.starts_with("ORCHESTRATION PRODUCT INCIDENT"));
    let delivered =
        rejected_report_delivery_message(failure.report_outcome, &failure.cause, "evidence");
    assert!(delivered.contains("Owner action required: ORCHESTRATION PRODUCT INCIDENT"));
}

fn add_task(store: &PlanStore, plan_id: &str, name: &str, now_ms: u64) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["report persists".into()],
                test_plan: Some("focused regression".into()),
                ..Default::default()
            },
            now_ms,
        )
        .unwrap()
}

#[test]
fn late_bound_no_id_report_persists_exact_task_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "late binding".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&store, &plan.id, "fresh dispatch", 2);
    assert_eq!(
        store
            .assigned_task_binding_projection("worker")
            .unwrap()
            .map(|binding| (binding.plan_id, binding.task_id)),
        None
    );
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "worker",
            None,
            Some("worker-session"),
            Some("worker-nonce"),
            None,
            4,
        )
        .unwrap();
    // A bound report carries the caller's own authoritative identity; the
    // refusal below is the product incident, not a default to fill in.
    let details = ReportDetails {
        caller_tab_id: Some("worker".into()),
        caller_session_id: Some("worker-session".into()),
        session_binding_nonce: Some("worker-nonce".into()),
        owner_epoch: Some(0),
        ..Default::default()
    };

    persist_local_task_report(
        &store,
        "worker",
        &plan.id,
        &task_id,
        &AgentRef::from_tab("worker"),
        "LATE_BIND_MARKER",
        &details,
        None,
        None,
        5,
    )
    .unwrap();

    let saved = store.get(&plan.id).unwrap();
    let task = saved.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.milestones.len(), 1);
    assert_eq!(task.milestones[0].kind, "worker_report");
    assert_eq!(task.milestones[0].text, "LATE_BIND_MARKER");
    assert_eq!(task.milestones[0].from.tab_id.as_deref(), Some("worker"));
}

#[test]
fn explicit_target_wins_over_discovered_binding() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut bound_plan = Plan::new(
        "bound".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch-a"),
    );
    store.save(&mut bound_plan, 1).unwrap();
    let bound_task = add_task(&store, &bound_plan.id, "bound task", 2);
    store
        .assign_task(&bound_plan.id, &bound_task, "worker", None, 3)
        .unwrap();

    let mut explicit_plan = Plan::new(
        "explicit".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch-b"),
    );
    store.save(&mut explicit_plan, 4).unwrap();
    let explicit_task = add_task(&store, &explicit_plan.id, "explicit task", 5);

    persist_local_task_report(
        &store,
        "worker",
        &explicit_plan.id,
        &explicit_task,
        &AgentRef::from_tab("worker"),
        "EXPLICIT_MARKER",
        &ReportDetails::default(),
        None,
        None,
        6,
    )
    .unwrap();

    let bound = store.get(&bound_plan.id).unwrap();
    assert!(bound.tasks[0].milestones.is_empty());
    let explicit = store.get(&explicit_plan.id).unwrap();
    assert_eq!(explicit.tasks[0].milestones[0].text, "EXPLICIT_MARKER");
}

#[test]
fn blocked_explicit_report_persists_as_proposal_without_task_transition() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "explicit blocked report".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&store, &plan.id, "blocked task", 2);
    let reason = "explicit worker report reason";
    store
        .edit_plan_fixture(&plan.id, 3, |plan| {
            plan.tasks[0].status = TaskStatus::InProgress;
            Ok(())
        })
        .unwrap();
    let details = ReportDetails {
        status: Some(ReportStatus::Blocked),
        ..Default::default()
    };

    persist_local_task_report(
        &store,
        "worker",
        &plan.id,
        &task_id,
        &AgentRef::from_tab("worker"),
        reason,
        &details,
        Some("blocked"),
        Some("worker_report"),
        4,
    )
    .unwrap();

    let task = store.get(&plan.id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.milestones.len(), 1);
    assert!(task.milestones[0].text.starts_with("BLOCK PROPOSAL: "));
    assert!(task.milestones[0].text.contains(reason));
}

#[test]
fn blocked_explicit_report_on_in_review_task_is_kept_as_a_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "explicit blocked review note".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&store, &plan.id, "reviewed task", 2);
    store
        .edit_plan_fixture(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InReview;
            Ok(())
        })
        .unwrap();

    let details = ReportDetails {
        status: Some(ReportStatus::Blocked),
        ..Default::default()
    };
    persist_local_task_report(
        &store,
        "worker",
        &plan.id,
        &task_id,
        &AgentRef::from_tab("worker"),
        "review still owns this task",
        &details,
        Some("blocked"),
        Some("worker_report"),
        4,
    )
    .unwrap();

    let task = store.get(&plan.id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.milestones.len(), 1);
    assert_eq!(
        task.milestones[0].text,
        "BLOCK PROPOSAL: review still owns this task"
    );
}

#[test]
fn missing_task_persistence_is_a_loud_incident_not_parent_only_success() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "persistence guard".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&store, &plan.id, "guarded task", 2);
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();

    let error = persist_local_task_report(
        &store,
        "worker",
        &plan.id,
        &task_id,
        &AgentRef::from_tab("wrong-reporter"),
        "MUST_NOT_DISAPPEAR",
        &ReportDetails::default(),
        None,
        None,
        4,
    )
    .unwrap_err();

    assert!(error.cause.contains("ORCHESTRATION PRODUCT INCIDENT"));
    assert!(error.cause.contains("parent delivery alone cannot prove"));
    assert!(error.cause.contains("no task-report success was returned"));
    assert!(store.get(&plan.id).unwrap().tasks[0].milestones.is_empty());
}

#[test]
fn spoofed_or_rebound_report_rejects_transition_without_mutating_task() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("reports".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("new-session".into());
        tab.session_binding_nonce = Some("new-nonce".into());
        tab.reported_session_binding_nonce = Some("new-nonce".into());
    }
    let mut plan = Plan::new(
        "reports".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&fixture.service.plan_store, &plan.id, "report", 2);
    let details = ReportDetails {
        caller_tab_id: Some(tab_id.clone()),
        caller_session_id: Some("old-session".into()),
        session_binding_nonce: Some("old-nonce".into()),
        owner_epoch: Some(1),
        ..Default::default()
    };
    let error = persist_authorized_local_task_report(
        &fixture.service,
        &tab_id,
        &plan.id,
        &task_id,
        &AgentRef::from_tab(&tab_id),
        "must still deliver outside this persistence seam",
        &details,
        None,
        None,
        3,
    )
    .unwrap_err();
    assert!(error.cause.contains("stale, spoofed, or rebound"));
    assert!(fixture
        .service
        .plan_store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .milestones
        .is_empty());
}

#[test]
fn verified_nonce_resolves_missing_report_session_id() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("reports".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("live-session".into());
        tab.session_binding_nonce = Some("live-nonce".into());
        tab.reported_session_binding_nonce = Some("live-nonce".into());
    }
    let mut plan = Plan::new(
        "reports".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&fixture.service.plan_store, &plan.id, "report", 2);
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &tab_id, None, 3)
        .unwrap();
    fixture
        .service
        .plan_store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            &tab_id,
            None,
            Some("live-session"),
            Some("live-nonce"),
            None,
            4,
        )
        .unwrap();
    let details = ReportDetails {
        caller_tab_id: Some(tab_id.clone()),
        caller_session_id: None,
        session_binding_nonce: Some("live-nonce".into()),
        owner_epoch: Some(0),
        ..Default::default()
    };
    persist_authorized_local_task_report(
        &fixture.service,
        &tab_id,
        &plan.id,
        &task_id,
        &AgentRef::from_tab(&tab_id),
        "report persisted",
        &details,
        None,
        None,
        5,
    )
    .unwrap();
    assert_eq!(
        fixture.service.plan_store.get(&plan.id).unwrap().tasks[0].milestones[0].text,
        "report persisted"
    );
}

#[test]
fn current_tab_binding_resolves_missing_report_identity_before_persistence() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("reports".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("live-session".into());
        tab.session_binding_nonce = Some("live-nonce".into());
        tab.reported_session_binding_nonce = Some("live-nonce".into());
    }
    let mut plan = Plan::new(
        "reports".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&fixture.service.plan_store, &plan.id, "report", 2);
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &tab_id, None, 3)
        .unwrap();
    fixture
        .service
        .plan_store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            &tab_id,
            None,
            Some("live-session"),
            Some("live-nonce"),
            None,
            4,
        )
        .unwrap();
    let details = ReportDetails {
        caller_tab_id: Some(tab_id.clone()),
        owner_epoch: Some(0),
        ..Default::default()
    };
    persist_authorized_local_task_report(
        &fixture.service,
        &tab_id,
        &plan.id,
        &task_id,
        &AgentRef::from_tab(&tab_id),
        "report persisted from current tab binding",
        &details,
        None,
        None,
        5,
    )
    .unwrap();
    assert_eq!(
        fixture.service.plan_store.get(&plan.id).unwrap().tasks[0].milestones[0].text,
        "report persisted from current tab binding"
    );
}
