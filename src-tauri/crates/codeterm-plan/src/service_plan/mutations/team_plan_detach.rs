use super::*;

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
                let retired_owner = OrchestratorRef::default();
                if plan.owner != retired_owner {
                    plan.owner_history.push(TransferRecord {
                        from: plan.owner.clone(),
                        to: retired_owner.clone(),
                        ts: now_ms,
                        reason: format!(
                            "Team {} closed",
                            plan.team_id.as_deref().unwrap_or_default()
                        ),
                    });
                }
                plan.owner = retired_owner;
                plan.planner = None;
                plan.team_id = None;
                plan.status = PlanStatus::Archived;
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
