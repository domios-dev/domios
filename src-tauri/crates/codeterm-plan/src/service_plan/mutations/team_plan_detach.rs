use super::*;
use crate::plan::{AgentRef, Milestone, PlanOrphan, TaskStatus};
use crate::service_plan::store_db::hot_write::lifecycle_rows::{
    self, LifecyclePlanRows, LifecycleUndo,
};
use std::collections::HashSet;

#[derive(Debug)]
pub struct TeamPlanArchive {
    undo: LifecycleUndo,
    changed_plan_ids: Vec<String>,
}
impl TeamPlanArchive {
    pub fn changed_plan_ids(&self) -> &[String] {
        &self.changed_plan_ids
    }
}

impl PlanStore {
    pub fn restore_close_plan_metadata(
        &self,
        saved: &crate::service_plan::PlanCloseRestoreRead,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let selector = PlanMatch::Ids(vec![saved.id.clone()]);
        let before = self
            .database
            .observe_team_rows(&selector, None, &[], false)?;
        let mut after = before.clone();
        let current = after.first_mut().ok_or(PlanError::NotFound)?;
        if current.team_id.is_some() && current.team_id != saved.team_id {
            return Err(PlanError::Io(format!(
                "Plan {} belongs to another Team",
                saved.id
            )));
        }
        if current.owner.tab_id.is_some() && current.owner.tab_id != saved.owner.tab_id {
            return Err(PlanError::Io(format!(
                "Plan {} has another Orchestrator",
                saved.id
            )));
        }
        current.team_id = saved.team_id.clone();
        current.owner = saved.owner.clone();
        current.planner = saved.planner.clone();
        current.status = saved.status.clone();
        current.orphaned = None;
        self.commit_team_row_patches(&turn, selector, before, after, now_ms)?;
        Ok(())
    }

    pub fn close_team_task_attempts_transaction(
        &self,
        team_id: &str,
        member_tab_ids: &[String],
        now_ms: u64,
    ) -> Result<TeamPlanArchive, PlanError> {
        let turn = self.mutation_writer.enter();
        let selector = super::super::store_queries::team_close_candidates(team_id, member_tab_ids);
        let plans =
            self.database
                .observe_team_rows(&selector, Some(team_id), member_tab_ids, true)?;
        let tabs: HashSet<_> = member_tab_ids.iter().map(String::as_str).collect();
        let mut staged = plans.clone();
        for plan in &mut staged {
            let bound = plan.team_id.as_deref() == Some(team_id);
            close_team_tasks(plan, team_id, &tabs, bound, now_ms)?;
            if bound {
                detach_closed_team_plan(plan, team_id, now_ms);
            }
        }
        self.commit_team_row_patches(&turn, selector, plans, staged, now_ms)
    }

    pub fn archive_and_detach_team_plans(
        &self,
        team_id: &str,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        Ok(self
            .archive_and_detach_team_plans_transaction(team_id, now_ms)?
            .changed_plan_ids)
    }

    pub fn archive_and_detach_team_plans_transaction(
        &self,
        team_id: &str,
        now_ms: u64,
    ) -> Result<TeamPlanArchive, PlanError> {
        let turn = self.mutation_writer.enter();
        let team_id = team_id.trim();
        let selector = PlanMatch::team(team_id);
        let plans = self
            .database
            .observe_team_rows(&selector, Some(team_id), &[], false)?;
        let mut staged = plans.clone();
        for plan in &mut staged {
            detach_closed_team_plan(plan, team_id, now_ms);
        }
        self.commit_team_row_patches(&turn, selector, plans, staged, now_ms)
    }

    pub fn rollback_team_plan_archive(
        &self,
        transaction: TeamPlanArchive,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let receipts = self.database.rollback_lifecycle_rows(transaction.undo)?;
        for receipt in receipts {
            self.publish_hot_receipt(&turn, receipt);
        }
        Ok(())
    }

    pub fn archive_active_plans_without_live_team(
        &self,
        live: &std::collections::BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let turn = self.mutation_writer.enter();
        let selector = PlanMatch::Status(PlanStatus::Active);
        let plans = self
            .database
            .observe_team_rows(&selector, None, &[], false)?;
        let mut active_by_team =
            std::collections::BTreeMap::<String, Vec<&LifecyclePlanRows>>::new();
        let mut archive_ids = std::collections::BTreeSet::new();
        for plan in &plans {
            if plan.team_id.is_none() && plan.orphaned.is_some() {
                continue;
            }
            match plan.team_id.as_deref() {
                Some(team) if live.contains(team) => {
                    active_by_team.entry(team.into()).or_default().push(plan)
                }
                _ => {
                    archive_ids.insert(plan.id.clone());
                }
            }
        }
        for plans in active_by_team.values_mut() {
            plans.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            archive_ids.extend(plans.iter().skip(1).map(|plan| plan.id.clone()));
        }
        let mut staged = plans.clone();
        for plan in &mut staged {
            if !archive_ids.contains(&plan.id) {
                continue;
            }
            if plan
                .team_id
                .as_ref()
                .is_none_or(|team| !live.contains(team))
            {
                plan.team_id = None;
            }
            plan.status = PlanStatus::Archived;
        }
        self.commit_team_row_patches(&turn, selector, plans, staged, now_ms)?;
        Ok(archive_ids.into_iter().collect())
    }

    pub(in crate::service_plan) fn commit_team_row_patches(
        &self,
        turn: &MutationWriterTurn<'_>,
        selector: PlanMatch,
        plans: Vec<LifecyclePlanRows>,
        staged: Vec<LifecyclePlanRows>,
        now_ms: u64,
    ) -> Result<TeamPlanArchive, PlanError> {
        turn.assert_store(self);
        let expected = lifecycle_rows::expected(&plans);
        let patches = plans
            .into_iter()
            .zip(staged)
            .filter_map(|(before, after)| lifecycle_rows::patch(before, after, now_ms))
            .collect();
        let (undo, receipts) =
            self.database
                .commit_lifecycle_rows(&selector, &expected, &Vec::new(), patches)?;
        let changed_plan_ids = undo
            .patches
            .iter()
            .map(|patch| patch.before.id.clone())
            .collect();
        for receipt in receipts {
            self.publish_hot_receipt(turn, receipt);
        }
        Ok(TeamPlanArchive {
            undo,
            changed_plan_ids,
        })
    }
}

fn close_team_tasks(
    plan: &mut LifecyclePlanRows,
    team_id: &str,
    tabs: &HashSet<&str>,
    include_orphans: bool,
    now_ms: u64,
) -> Result<(), PlanError> {
    let mut affected_tasks = 0;
    let mut reset_tasks = 0;
    let mut closed_attempts = 0;
    for task in &mut plan.tasks {
        let mut task_attempts_closed = 0;
        for attempt in &mut task.execution_attempts {
            if tabs.contains(attempt.executor_tab_id.as_str()) && attempt.closed_at.is_none() {
                attempt.closed_at = Some(now_ms);
                attempt.retired_at.get_or_insert(now_ms);
                attempt.closure_reason = Some("team_closed".into());
                task_attempts_closed += 1;
            }
        }
        let assigned_to_closed_tab = task
            .assigned_tab_id
            .as_deref()
            .is_some_and(|tab_id| tabs.contains(tab_id))
            || task
                .lane
                .tab_id
                .as_deref()
                .is_some_and(|tab_id| tabs.contains(tab_id));
        let orphaned_in_progress = include_orphans
            && task.status == TaskStatus::InProgress
            && task.assigned_tab_id.is_none()
            && task.lane.tab_id.is_none();
        let reset_task = assigned_to_closed_tab || orphaned_in_progress || task_attempts_closed > 0;
        if task_attempts_closed == 0 && !reset_task {
            continue;
        }
        closed_attempts += task_attempts_closed;
        affected_tasks += 1;
        let preserve_review = task.status == TaskStatus::InReview;
        let returned_to_pending = reset_task
            && !task.status.is_terminal()
            && !preserve_review
            && task.status != TaskStatus::Pending;
        if returned_to_pending {
            reset_tasks += 1;
        }
        if reset_task && !task.status.is_terminal() && !preserve_review {
            if task.status != TaskStatus::Pending {
                task.status = TaskStatus::Pending;
                task.status_changed_at = now_ms;
            }
            task.status_label =
                returned_to_pending.then(|| crate::plan::reconcile::TEAM_CLOSED_LABEL.to_string());
            task.pause_reason = None;
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.taken_by_role = None;
            task.interruption.owner_epoch =
                super::super::interruption::next_owner_epoch(task.interruption.owner_epoch)?;
            task.interruption.owner_tab_id = None;
            task.interruption.owner_session_id = None;
            task.interruption.handoff_required = false;
            crate::service_plan::qa::normalize_task_review_fields(task);
        }
        task.updated_at = now_ms;
        task.milestones.push(Milestone {
            id: crate::plan::ids::short_id("ms"),
            from: AgentRef::default(),
            kind: "team_close".into(),
            text: if returned_to_pending {
                format!("Team {team_id} closed; task returned to pending.")
            } else if preserve_review {
                format!("Team {team_id} closed; finalized {task_attempts_closed} attempts; review remains open.")
            } else if reset_task && !task.status.is_terminal() {
                format!("Team {team_id} closed; cleared the pending task assignment.")
            } else {
                format!("Team {team_id} closed; finalized {task_attempts_closed} attempts.")
            },
            ts: now_ms,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
    }
    if affected_tasks > 0 {
        plan.strides.push(Milestone {
            id: crate::plan::ids::short_id("ms"),
            from: AgentRef::default(),
            kind: "team_close".into(),
            text: format!(
                "Team {team_id} closed; returned {reset_tasks} tasks to pending and finalized {closed_attempts} attempts across {affected_tasks} tasks."
            ),
            ts: now_ms,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
    }
    Ok(())
}

pub(super) fn detach_closed_team_plan(plan: &mut LifecyclePlanRows, team_id: &str, now_ms: u64) {
    let retired_owner = OrchestratorRef::default();
    if plan.owner != retired_owner {
        plan.owner_history.push(TransferRecord {
            from: plan.owner.clone(),
            to: retired_owner.clone(),
            ts: now_ms,
            reason: format!("Team {team_id} closed"),
        });
    }
    plan.owner = retired_owner;
    plan.planner = None;
    plan.team_id = None;
    let unfinished_task_ids = plan.unfinished_task_ids.clone();
    if unfinished_task_ids.is_empty() {
        plan.status = PlanStatus::Archived;
        return;
    }
    plan.status = PlanStatus::Active;
    plan.orphaned = Some(PlanOrphan {
        team_id: team_id.to_string(),
        reason: format!(
            "Team {team_id} closed with {} unfinished task(s): {}; adopt the plan into a live Team to continue",
            unfinished_task_ids.len(),
            unfinished_task_ids.join(", ")
        ),
        unfinished_task_ids,
        at: now_ms,
    });
}

#[cfg(test)]
mod close_restore_tests;
