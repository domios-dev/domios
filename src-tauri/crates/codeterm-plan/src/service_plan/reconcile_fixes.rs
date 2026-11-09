use super::store_db::hot_write::TaskHistory;
use super::{task_attempts, PlanError, PlanStore, ReconcileFixResult};
use crate::plan::reconcile::{
    DriftItem, DriftKind, LaneDemotion, NEEDS_REASSIGN_LABEL, STALE_REVIEW_LABEL,
    STALE_UNSTAFFED_LABEL, TAB_GONE_LABEL,
};
use crate::plan::{AgentRef, Milestone, TaskStatus};

impl PlanStore {
    pub fn apply_reconcile_fixes(
        &self,
        plan_id: &str,
        items: &[DriftItem],
        now_ms: u64,
    ) -> Result<Vec<ReconcileFixResult>, PlanError> {
        let items: Vec<_> = items.iter().filter(|item| !item.not_mine).collect();
        let ids: Vec<_> = items.iter().map(|item| item.task_id.clone()).collect();
        self.with_task_rows_batch(
            plan_id,
            &ids,
            TaskHistory::LiveAndReported,
            now_ms,
            |index, edit| {
                let item = items[index];
                let task = &mut edit.task;
                let action = match item.kind {
                    DriftKind::AcceptedButNotDone => {
                        edit.terminal_check = true;
                        edit.close_all_live = true;
                        edit.review_credit = i64::from(task.status == TaskStatus::InReview);
                        if task.status != TaskStatus::Done {
                            task.status_changed_at = now_ms;
                        }
                        task.status = TaskStatus::Done;
                        super::qa::normalize_task_review_fields(task);
                        super::mutations::clear_task_assignment(task);
                        "set_done"
                    }
                    DriftKind::InProgressTabGone | DriftKind::BoundToClosedTab => {
                        let assigned = task.assigned_tab_id.clone();
                        task_attempts::mark_task_assignment_closed(
                            task,
                            assigned.as_deref(),
                            now_ms,
                        );
                        task.assigned_tab_id = None;
                        task.lane.tab_id = None;
                        if item.kind == DriftKind::InProgressTabGone {
                            task.status_label = Some(NEEDS_REASSIGN_LABEL.into());
                            "cleared_assignment_set_needs_reassign"
                        } else {
                            "cleared_dead_assignment"
                        }
                    }
                    DriftKind::StaleInReview => {
                        task.status_label = Some(STALE_REVIEW_LABEL.into());
                        "set_stale_review_label"
                    }
                    DriftKind::StalePendingUnstaffed => {
                        task.status_label = Some(STALE_UNSTAFFED_LABEL.into());
                        "set_stale_unstaffed_label"
                    }
                    DriftKind::ReportTargetWriteMissing => return Ok(None),
                };
                task.updated_at = now_ms;
                Ok(Some(fix_result(plan_id, item, &task.id, action)))
            },
        )
    }

    pub fn apply_lane_demotions(
        &self,
        plan_id: &str,
        demotions: &[LaneDemotion],
        now_ms: u64,
    ) -> Result<Vec<ReconcileFixResult>, PlanError> {
        let ids: Vec<_> = demotions
            .iter()
            .map(|demotion| demotion.task_id.clone())
            .collect();
        self.with_task_rows_batch(
            plan_id,
            &ids,
            TaskHistory::LiveAndReported,
            now_ms,
            |index, edit| {
                let task = &mut edit.task;
                if task.status != TaskStatus::InProgress {
                    return Ok(None);
                }
                let assigned = task.assigned_tab_id.clone();
                task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
                task.assigned_tab_id = None;
                task.lane.tab_id = None;
                task.status_label = Some(NEEDS_REASSIGN_LABEL.into());
                task.updated_at = now_ms;
                task.milestones.push(Milestone {
                    id: crate::plan::ids::short_id("ms"),
                    from: AgentRef::from_tab("watchdog"),
                    kind: "decision".into(),
                    text: demotions[index].reason.clone(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
                Ok(Some(fix_result_action(
                    plan_id,
                    &task.id,
                    DriftKind::InProgressTabGone,
                    "cleared_assignment_set_needs_reassign",
                )))
            },
        )
    }

    pub fn flag_tab_gone(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(plan_id, task_id, TaskHistory::Attempts, now_ms, |edit| {
            let task = &mut edit.task;
            if task.id != task_id {
                return Ok(false);
            }
            let retired = super::task_recovery::retire_attempts_for_tab(task, tab_id, now_ms);
            let closed = task_attempts::mark_task_assignment_closed(task, Some(tab_id), now_ms) > 0
                || !retired.is_empty();
            if task.assigned_tab_id.as_deref() == Some(tab_id) {
                task.assigned_tab_id = None;
                task.lane.tab_id = None;
                task.status_label = Some(TAB_GONE_LABEL.into());
                task.updated_at = now_ms;
                return Ok(true);
            }
            Ok(closed)
        })
        .or_else(|error| match error {
            PlanError::TaskNotFound => Ok(false),
            error => Err(error),
        })
    }

    pub fn close_gone_task_attempts(
        &self,
        plan_id: &str,
        gone: &[(String, String)],
        now_ms: u64,
    ) -> Result<Vec<ReconcileFixResult>, PlanError> {
        let ids: Vec<_> = gone.iter().map(|(id, _)| id.clone()).collect();
        self.with_task_rows_batch(
            plan_id,
            &ids,
            TaskHistory::Attempts,
            now_ms,
            |index, edit| {
                let task = &mut edit.task;
                let tab_id = &gone[index].1;
                let retired = super::task_recovery::retire_attempts_for_tab(task, tab_id, now_ms);
                if task_attempts::mark_task_assignment_closed(task, Some(tab_id), now_ms) == 0
                    && retired.is_empty()
                {
                    return Ok(None);
                }
                Ok(Some(fix_result_action(
                    plan_id,
                    &task.id,
                    DriftKind::BoundToClosedTab,
                    "closed_dead_attempt",
                )))
            },
        )
    }
}

fn fix_result(plan_id: &str, item: &DriftItem, task_id: &str, action: &str) -> ReconcileFixResult {
    fix_result_action(plan_id, task_id, item.kind.clone(), action)
}

fn fix_result_action(
    plan_id: &str,
    task_id: &str,
    kind: DriftKind,
    action: &str,
) -> ReconcileFixResult {
    ReconcileFixResult {
        plan_id: plan_id.into(),
        task_id: task_id.into(),
        kind,
        action: action.into(),
    }
}
