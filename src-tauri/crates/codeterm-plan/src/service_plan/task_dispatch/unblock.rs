use super::super::PlanError;

pub(super) fn task_startup_status(
    status: &crate::plan::TaskStatus,
) -> Result<crate::plan::TaskStatus, PlanError> {
    match status {
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress => {
            Ok(status.clone())
        }
        crate::plan::TaskStatus::Blocked => Ok(crate::plan::TaskStatus::Pending),
        _ => Err(PlanError::TransitionDenied {
            from: status.clone(),
            to: crate::plan::TaskStatus::Pending,
        }),
    }
}

pub(super) fn record_blocked_task_unblock(
    plan: &mut crate::plan::Plan,
    task_index: usize,
    action: &str,
    now_ms: u64,
) {
    if plan.tasks[task_index].status != crate::plan::TaskStatus::Blocked {
        return;
    }
    let task_id = plan.tasks[task_index].id.clone();
    let reason = plan.tasks[task_index]
        .status_label
        .clone()
        .unwrap_or_else(|| "blocked without a recorded reason".into());
    let stride_id = crate::plan::ids::mint_id("ms", |candidate| {
        super::super::mutations::plan_contains_id(plan, candidate)
    });
    plan.strides.push(crate::plan::Milestone {
        id: stride_id,
        from: plan.owner.clone(),
        kind: "decision".into(),
        text: format!("unblocked by {action}: {reason}"),
        ts: now_ms,
        links: vec![task_id],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    });
    let task = &mut plan.tasks[task_index];
    task.status = crate::plan::TaskStatus::Pending;
    task.status_changed_at = now_ms;
}
