use super::*;

/// rel#67: `bounce` has an extra requirement past task resolution — an exact,
/// single, resolvable continuation attempt. When that extra requirement fails
/// for any reason, the task itself still resolved (`show`/`hold`/`accept`
/// would find it), so the error must never collapse to `TaskNotFound`.
#[test]
fn bounce_review_names_the_exact_attempt_gap_not_a_missing_task() {
    fn assert_not_task_not_found(store: &PlanStore, plan_id: &str, task_id: &str, label: &str) {
        let err = store
            .bounce_review(
                plan_id,
                task_id,
                "attempted bounce",
                false,
                false,
                AgentRef::from_tab("reviewer"),
                100,
            )
            .unwrap_err();
        assert!(
            !matches!(err, PlanError::TaskNotFound),
            "{label}: bounce must not report an existing task as TaskNotFound, got {err:?}"
        );
        assert!(
            matches!(err, PlanError::ReviewContinuationAttemptNotFound { .. }),
            "{label}: expected ReviewContinuationAttemptNotFound, got {err:?}"
        );
    }

    // 1. No execution attempts at all.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "no-attempts", 2);
        set_status(&store, &plan_id, &task_id, "in_review", 3).unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "no attempts");
    }

    // 2. The sole attempt is already closed.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "closed-attempt", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.execution_attempts.last_mut().unwrap().closed_at = Some(5);
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "closed attempt");
    }

    // 3. Not acknowledged.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "unacked", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.execution_attempts.last_mut().unwrap().acknowledged_at = None;
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "unacknowledged attempt");
    }

    // 4. Not reported.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "unreported", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.execution_attempts.last_mut().unwrap().reported_at = None;
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "unreported attempt");
    }

    // 5. Missing session id.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "no-session", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.execution_attempts.last_mut().unwrap().session_id = None;
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "missing session id");
    }

    // 6. Non-competition attempt no longer matches the task's assigned tab.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "reassigned", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                task.assigned_tab_id = Some("someone_else".into());
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(&store, &plan_id, &task_id, "reassigned tab mismatch");
    }

    // 7. Competition attempt that never won.
    {
        let (_dir, store, plan_id) = setup();
        let task_id = add_ready_task(&store, &plan_id, "competed", 2);
        dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
        set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
                let attempt = task.execution_attempts.last_mut().unwrap();
                attempt.competition_group_id = Some("group_1".into());
                attempt.winner = false;
                Ok(())
            })
            .unwrap();
        assert_not_task_not_found(
            &store,
            &plan_id,
            &task_id,
            "non-winning competition attempt",
        );
    }
}
