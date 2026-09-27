use super::*;

#[test]
fn unplanned_task_in_review_report_moves_to_in_review() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Unplanned report".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "unplanned work".into(),
                kind: "unplanned".into(),
                acceptance_criteria: vec!["the brief is satisfied".into()],
                test_plan: Some(
                    "Orchestrator verifies the reported result against the brief".into(),
                ),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    mark_bound_task_in_progress(&store, &plan.id, &task_id);

    assert!(store
        .ingest_bound_worker_report(
            &plan.id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap());

    assert_eq!(
        store.get(&plan.id).unwrap().tasks[0].status,
        TaskStatus::InReview
    );
}

#[test]
fn planned_task_without_qa_plan_is_still_refused() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .with_plan(&plan_id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.acceptance_criteria.clear();
            task.test_plan = None;
            Ok(())
        })
        .unwrap();

    let error = store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, None),
            4,
        )
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        crate::service_plan::PlanError::ReviewEvidenceRequired.to_string()
    );
    assert_eq!(
        store.get(&plan_id).unwrap().tasks[0].status,
        TaskStatus::Pending
    );
}

#[test]
fn unplanned_partial_and_blocked_reports_follow_task_status_mapping() {
    for (status, expected) in [
        (ReportStatus::Partial, TaskStatus::InProgress),
        (ReportStatus::Blocked, TaskStatus::Blocked),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Unplanned report mapping".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "unplanned mapping".into(),
                    kind: "unplanned".into(),
                    acceptance_criteria: vec!["brief is satisfied".into()],
                    test_plan: Some("Orchestrator verifies the reported result".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store
            .assign_task(&plan.id, &task_id, "worker", None, 3)
            .unwrap();
        mark_bound_task_in_progress(&store, &plan.id, &task_id);
        let report = worker_report(status, None);

        assert!(!store
            .ingest_bound_worker_report(
                &plan.id,
                &task_id,
                &AgentRef::from_tab("worker"),
                report,
                4,
            )
            .unwrap());
        assert_eq!(store.get(&plan.id).unwrap().tasks[0].status, expected);
    }
}
