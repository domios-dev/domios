use crate::plan::ids::mint_id;
use crate::plan::reconcile::{
    DriftItem, DriftKind, LaneDemotion, NEEDS_REASSIGN_LABEL, TAB_GONE_LABEL, STALE_REVIEW_LABEL,
    STALE_UNSTAFFED_LABEL,
};
use crate::plan::{AgentRef, Milestone, TaskStatus};

use super::mutations::plan_contains_id;
use super::{task_attempts, PlanError, PlanStore, ReconcileFixResult};

impl PlanStore {
    pub fn apply_reconcile_fixes(
        &self,
        plan_id: &str,
        items: &[DriftItem],
        now_ms: u64,
    ) -> Result<Vec<ReconcileFixResult>, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let mut changed = Vec::new();
            for item in items.iter().filter(|item| !item.not_mine) {
                let Some(task_index) = plan.tasks.iter().position(|task| task.id == item.task_id)
                else {
                    continue;
                };
                if matches!(item.kind, DriftKind::AcceptedButNotDone) {
                    let task_id = plan.tasks[task_index].id.clone();
                    let was_in_review = plan.tasks[task_index].status == TaskStatus::InReview;
                    super::mutations::finalize_task_done(plan, task_index, now_ms)?;
                    if was_in_review {
                        super::review_gate::earn_review_credit(&mut plan.review_policy);
                    }
                    changed.push(fix_result(plan_id, item, &task_id, "set_done"));
                    continue;
                }
                let task = &mut plan.tasks[task_index];
                match item.kind {
                    DriftKind::InProgressTabGone => {
                        let assigned = task.assigned_tab_id.clone();
                        task_attempts::mark_task_assignment_closed(
                            task,
                            assigned.as_deref(),
                            now_ms,
                        );
                        task.assigned_tab_id = None;
                        task.lane.tab_id = None;
                        task.status_label = Some(NEEDS_REASSIGN_LABEL.into());
                        task.updated_at = now_ms;
                        changed.push(fix_result(
                            plan_id,
                            item,
                            &task.id,
                            "cleared_assignment_set_needs_reassign",
                        ));
                    }
                    DriftKind::StaleInReview => {
                        task.status_label = Some(STALE_REVIEW_LABEL.into());
                        task.updated_at = now_ms;
                        changed.push(fix_result(
                            plan_id,
                            item,
                            &task.id,
                            "set_stale_review_label",
                        ));
                    }
                    DriftKind::StalePendingUnstaffed => {
                        task.status_label = Some(STALE_UNSTAFFED_LABEL.into());
                        task.updated_at = now_ms;
                        changed.push(fix_result(
                            plan_id,
                            item,
                            &task.id,
                            "set_stale_unstaffed_label",
                        ));
                    }
                    DriftKind::BoundToClosedTab => {
                        let assigned = task.assigned_tab_id.clone();
                        task_attempts::mark_task_assignment_closed(
                            task,
                            assigned.as_deref(),
                            now_ms,
                        );
                        task.assigned_tab_id = None;
                        task.lane.tab_id = None;
                        task.updated_at = now_ms;
                        changed.push(fix_result(
                            plan_id,
                            item,
                            &task.id,
                            "cleared_dead_assignment",
                        ));
                    }
                    DriftKind::AcceptedButNotDone => unreachable!(),
                }
            }
            Ok(changed)
        })
    }

    /// Apply watchdog demotions: an in_progress lane whose tab is gone past the
    /// grace window is unassigned, marked needs-reassign, and given ONE stride.
    /// Idempotent — a lane already off in_progress is skipped, so re-running the
    /// tick never duplicates the stride or silently drifts the status to pending.
    pub fn apply_lane_demotions(
        &self,
        plan_id: &str,
        demotions: &[LaneDemotion],
        now_ms: u64,
    ) -> Result<Vec<ReconcileFixResult>, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let mut changed = Vec::new();
            for demotion in demotions {
                let mid = mint_id("ms", |candidate| plan_contains_id(plan, candidate));
                let Some(task) = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == demotion.task_id)
                else {
                    continue;
                };
                if task.status != TaskStatus::InProgress {
                    continue;
                }
                let assigned = task.assigned_tab_id.clone();
                task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
                task.assigned_tab_id = None;
                task.lane.tab_id = None;
                task.status_label = Some(NEEDS_REASSIGN_LABEL.into());
                task.updated_at = now_ms;
                task.milestones.push(Milestone {
                    id: mid,
                    from: AgentRef::from_tab("watchdog"),
                    kind: "decision".into(),
                    text: demotion.reason.clone(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
                changed.push(fix_result_action(
                    plan_id,
                    &task.id,
                    DriftKind::InProgressTabGone,
                    "cleared_assignment_set_needs_reassign",
                ));
            }
            Ok(changed)
        })
    }

    pub fn flag_tab_gone(
        &self,
        plan_id: &str,
        task_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(task) = plan.tasks.iter_mut().find(|task| task.id == task_id) else {
                return Ok(false);
            };
            let assigned = task.assigned_tab_id.clone();
            task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.status_label = Some(TAB_GONE_LABEL.into());
            task.updated_at = now_ms;
            Ok(true)
        })
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
