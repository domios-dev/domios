use super::*;
use crate::plan::{AgentRef, Milestone, TaskStatus};
use std::collections::HashSet;

#[derive(Debug)]
pub struct TeamPlanArchive {
    original_plans: Vec<Plan>,
    changed_plan_ids: Vec<String>,
}

impl TeamPlanArchive {
    pub fn changed_plan_ids(&self) -> &[String] {
        &self.changed_plan_ids
    }
}

impl PlanStore {
    pub fn close_team_task_attempts_transaction(
        &self,
        team_id: &str,
        member_tab_ids: &[String],
        now_ms: u64,
    ) -> Result<TeamPlanArchive, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let tabs: HashSet<_> = member_tab_ids.iter().map(String::as_str).collect();
        let mut ids: Vec<_> = self
            .list_full()
            .into_iter()
            .filter(|plan| has_team_close_work(plan, team_id, &tabs))
            .map(|plan| plan.id)
            .collect();
        ids.sort();
        let originals: Vec<_> = ids.iter().filter_map(|id| self.get(id)).collect();
        for id in &ids {
            if let Err(error) = self.with_plan_in_writer(id, now_ms, |plan| {
                let team_bound = plan.team_id.as_deref() == Some(team_id);
                close_team_tasks(plan, team_id, &tabs, team_bound, now_ms);
                if team_bound {
                    detach_closed_team_plan(plan, team_id, now_ms);
                }
                Ok(())
            }) {
                for original in &originals {
                    if let Err(rollback_error) = self.restore_team_plan(original) {
                        log::error!(
                            "[plan.team-close] rollback failed for plan {} after {error}: {rollback_error}",
                            original.id
                        );
                    }
                }
                return Err(error);
            }
        }
        Ok(TeamPlanArchive {
            original_plans: originals,
            changed_plan_ids: ids,
        })
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
        let _writer_turn = self.mutation_writer.enter();
        let target_team_id = team_id.trim().to_string();
        self.archive_matching_team_plans(|bound| bound == target_team_id, now_ms)
    }

    pub fn rollback_team_plan_archive(
        &self,
        transaction: TeamPlanArchive,
    ) -> Result<(), PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        for plan in transaction.original_plans {
            self.restore_team_plan(&plan)?;
        }
        Ok(())
    }

    pub fn archive_active_plans_without_live_team(
        &self,
        live: &std::collections::BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let plans = self.list_full();
        let mut active_by_team = std::collections::BTreeMap::<String, Vec<&Plan>>::new();
        let mut archive_ids = Vec::new();
        for plan in plans
            .iter()
            .filter(|plan| plan.status == PlanStatus::Active)
        {
            match plan.team_id.as_deref() {
                Some(team_id) if live.contains(team_id) => {
                    active_by_team.entry(team_id.into()).or_default().push(plan);
                }
                _ => archive_ids.push(plan.id.clone()),
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
        archive_ids.sort();
        let originals: Vec<_> = archive_ids.iter().filter_map(|id| self.get(id)).collect();
        for id in &archive_ids {
            if let Err(error) = self.with_plan_in_writer(id, now_ms, |plan| {
                if plan
                    .team_id
                    .as_ref()
                    .is_none_or(|team_id| !live.contains(team_id))
                {
                    plan.team_id = None;
                }
                plan.status = PlanStatus::Archived;
                Ok(())
            }) {
                for original in &originals {
                    let _ = self.restore_team_plan(original);
                }
                return Err(error);
            }
        }
        Ok(archive_ids)
    }

    fn archive_matching_team_plans<F>(
        &self,
        matches: F,
        now_ms: u64,
    ) -> Result<TeamPlanArchive, PlanError>
    where
        F: Fn(&str) -> bool,
    {
        let mut ids: Vec<_> = self
            .list_full()
            .into_iter()
            .filter_map(|plan| {
                let bound_team = plan.team_id.as_deref();
                bound_team.filter(|team| matches(team)).map(|_| plan.id)
            })
            .collect();
        ids.sort();
        let original_plans: Vec<_> = ids.iter().filter_map(|id| self.get(id)).collect();
        for id in &ids {
            if let Err(error) = self.with_plan_in_writer(id, now_ms, |plan| {
                let team_id = plan.team_id.clone().unwrap_or_default();
                detach_closed_team_plan(plan, &team_id, now_ms);
                Ok(())
            }) {
                for original in &original_plans {
                    let _ = self.restore_team_plan(original);
                }
                return Err(error);
            }
        }
        Ok(TeamPlanArchive {
            original_plans,
            changed_plan_ids: ids,
        })
    }

    fn restore_team_plan(&self, original: &Plan) -> Result<(), PlanError> {
        let arc = self
            .plans
            .read()
            .get(&original.id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        let staged = original.clone();
        let path = self
            .persist_plan_and_spec(&staged)
            .map_err(|error| PlanError::Io(error.to_string()))?;
        {
            let mut plan = arc.lock();
            *plan = staged.clone();
        }
        self.note_plan_write(&staged, &path);
        self.index
            .write()
            .insert(staged.id.clone(), Self::index_entry(&staged, path));
        Ok(())
    }
}

fn has_team_close_work(plan: &Plan, team_id: &str, tabs: &HashSet<&str>) -> bool {
    plan.team_id.as_deref() == Some(team_id)
        || plan.tasks.iter().any(|task| {
            task.assigned_tab_id
                .as_deref()
                .is_some_and(|tab_id| tabs.contains(tab_id))
                || task
                    .lane
                    .tab_id
                    .as_deref()
                    .is_some_and(|tab_id| tabs.contains(tab_id))
                || task
                    .execution_attempts
                    .iter()
                    .any(|attempt| tabs.contains(attempt.executor_tab_id.as_str()))
                || (plan.team_id.as_deref() == Some(team_id)
                    && task.status == TaskStatus::InProgress
                    && task.assigned_tab_id.is_none()
                    && task.lane.tab_id.is_none())
        })
}

fn close_team_tasks(
    plan: &mut Plan,
    team_id: &str,
    tabs: &HashSet<&str>,
    include_orphans: bool,
    now_ms: u64,
) {
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
        let returned_to_pending =
            reset_task
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
            task.status_label = None;
            task.pause_reason = None;
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.taken_by_role = None;
            task.interruption.owner_epoch = task.interruption.owner_epoch.saturating_add(1);
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
}

fn detach_closed_team_plan(plan: &mut Plan, team_id: &str, now_ms: u64) {
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
    plan.status = PlanStatus::Archived;
}
