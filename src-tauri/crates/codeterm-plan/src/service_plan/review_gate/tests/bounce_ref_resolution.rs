use super::*;

/// rel#67: Missing or ambiguous attempts must not be reported as a missing task.
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

    // 2. The sole attempt is already closed, so the bounce reopens the task.
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
        let result = store
            .bounce_review(
                &plan_id,
                &task_id,
                "attempted bounce",
                false,
                false,
                AgentRef::from_tab("reviewer"),
                6,
            )
            .unwrap();
        assert!(result.continuation.is_none());
        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert_eq!(task.status, crate::plan::TaskStatus::Pending);
        assert_eq!(task.status_label.as_deref(), Some("review bounced"));
        assert!(task.execution_attempts[0].closed_at.is_some());
        assert!(task
            .milestones
            .last()
            .unwrap()
            .text
            .contains("attempted bounce"));
        let stride_path =
            crate::service_plan::body_files::task_stride_log_path(&plan, task).unwrap();
        let strides = crate::service_plan::stride_log::read_milestones(&stride_path).unwrap();
        assert!(strides
            .iter()
            .any(|entry| entry.text.contains("attempted bounce")));
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

#[test]
fn bounce_reopens_in_review_task_whose_executor_is_gone() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "closed-attempt", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let lane = crate::plan::TaskLaneMetadata {
        tab_id: Some("tab_impl".into()),
        workspace_id: Some("workspace_impl".into()),
        workspace_name: Some("implementation".into()),
        worktree_path: Some("D:/repo/.worktrees/task-1".into()),
        branch: Some("lane/task-1".into()),
        base: Some("main".into()),
        destination: Some("main".into()),
        ..Default::default()
    };
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = plan.tasks.iter_mut().find(|t| t.id == task_id).unwrap();
            task.lane = lane.clone();
            Ok(())
        })
        .unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "retry after the Worker exited",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            6,
        )
        .unwrap();

    let continuation = result.continuation.unwrap();
    store
        .reopen_review_continuation(
            &plan_id,
            &task_id,
            &continuation.attempt_id,
            continuation.epoch,
            7,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, crate::plan::TaskStatus::Pending);
    assert_eq!(task.status_label.as_deref(), Some("review bounced"));
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab_impl"));
    assert_eq!(task.lane, lane);
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].executor_tab_id, "tab_impl");
    assert_eq!(task.execution_attempts[0].closed_at, Some(7));
    assert!(task
        .review_evidence
        .last()
        .unwrap()
        .note
        .contains("retry after the Worker exited"));
    let milestone = task.milestones.last().unwrap();
    assert_eq!(milestone.kind, REVIEW_MILESTONE_KIND);
    assert!(milestone.text.contains("retry after the Worker exited"));
    let stride_path = crate::service_plan::body_files::task_stride_log_path(&plan, task).unwrap();
    let strides = crate::service_plan::stride_log::read_milestones(&stride_path).unwrap();
    assert!(strides.iter().any(|stride| {
        stride.id == milestone.id && stride.text.contains("retry after the Worker exited")
    }));
}

#[test]
fn bounce_reopens_a_closed_continuation_without_replacing_its_note() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "closed-continuation", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "preserve this review note",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    assert!(first.continuation.is_some());
    store
        .with_plan(&plan_id, 6, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.execution_attempts.last_mut().unwrap().closed_at = Some(6);
            Ok(())
        })
        .unwrap();

    let reopened = store
        .bounce_review(
            &plan_id,
            &task_id,
            "do not replace the first note",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            7,
        )
        .unwrap();

    assert!(reopened.continuation.is_none());
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, crate::plan::TaskStatus::Pending);
    assert_eq!(task.review_evidence.len(), 1);
    assert_eq!(task.review_evidence[0].note, "preserve this review note");
    assert_eq!(task.execution_attempts[0].closed_at, Some(6));
}

#[test]
fn bounce_still_refuses_multiple_live_review_attempts() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "ambiguous-attempts", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            let mut competing = task.execution_attempts[0].clone();
            competing.id = "attempt_competing".into();
            task.execution_attempts.push(competing);
            Ok(())
        })
        .unwrap();

    let error = store
        .bounce_review(
            &plan_id,
            &task_id,
            "attempted bounce",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            100,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PlanError::ReviewContinuationAttemptNotFound { .. }
    ));
}
