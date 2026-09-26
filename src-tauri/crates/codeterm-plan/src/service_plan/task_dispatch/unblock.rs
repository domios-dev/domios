use super::super::PlanError;

pub(in crate::service_plan) fn task_startup_status(
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

pub(in crate::service_plan) fn task_respawn_status(
    status: &crate::plan::TaskStatus,
) -> Result<crate::plan::TaskStatus, PlanError> {
    match status {
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress => {
            Ok(status.clone())
        }
        crate::plan::TaskStatus::Blocked | crate::plan::TaskStatus::InReview => {
            Ok(crate::plan::TaskStatus::Pending)
        }
        _ => Err(PlanError::TransitionDenied {
            from: status.clone(),
            to: crate::plan::TaskStatus::Pending,
        }),
    }
}

pub(in crate::service_plan) fn record_blocked_task_unblock(
    plan: &mut crate::plan::Plan,
    task_index: usize,
    action: &str,
    attempt_id: Option<&str>,
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
    let attempt_note = attempt_id
        .map(|attempt_id| format!("\nAttempt id: {attempt_id}"))
        .unwrap_or_default();
    plan.strides.push(crate::plan::Milestone {
        id: stride_id,
        from: plan.owner.clone(),
        kind: "decision".into(),
        text: format!("unblocked by {action}: {reason}{attempt_note}"),
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

pub(in crate::service_plan) fn restore_blocked_unblock(
    plan: &mut crate::plan::Plan,
    task_id: &str,
    attempt_id: &str,
) -> Option<String> {
    let marker = format!("\nAttempt id: {attempt_id}");
    let stride = plan.strides.iter_mut().rev().find(|stride| {
        stride.links.iter().any(|link| link == task_id)
            && stride.text.starts_with("unblocked by dispatch: ")
            && stride.text.ends_with(&marker)
    })?;
    let reason = stride
        .text
        .strip_prefix("unblocked by dispatch: ")?
        .strip_suffix(&marker)?
        .to_string();
    stride.text = format!("dispatch rolled back; task remains blocked: {reason}");
    Some(reason)
}

pub(in crate::service_plan) fn annotate_blocked_unblock_rollback(
    plan: &mut crate::plan::Plan,
    task_id: &str,
    attempt_id: &str,
) {
    let marker = format!("\nAttempt id: {attempt_id}");
    let prefixes = ["unblocked by dispatch: ", "unblocked by respawn: "];
    if let Some(stride) = plan.strides.iter_mut().rev().find(|stride| {
        stride.links.iter().any(|link| link == task_id)
            && prefixes.iter().any(|prefix| stride.text.starts_with(prefix))
            && stride.text.ends_with(&marker)
    }) {
        let reason = prefixes
            .iter()
            .find_map(|prefix| stride.text.strip_prefix(prefix))
            .and_then(|text| text.strip_suffix(&marker))
            .unwrap_or_default();
        stride.text = format!("startup rolled back; task remains blocked: {reason}");
    }
}
