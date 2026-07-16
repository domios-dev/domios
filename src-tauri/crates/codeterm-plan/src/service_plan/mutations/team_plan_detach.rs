use super::*;

#[derive(Debug)]
pub struct TeamPlanDetach {
    original_plans: Vec<Plan>,
    changed_plan_ids: Vec<String>,
}

impl TeamPlanDetach {
    pub fn changed_plan_ids(&self) -> &[String] {
        &self.changed_plan_ids
    }
}

impl PlanStore {
    pub fn detach_and_park_team_plans(
        &self,
        team_id: &str,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        Ok(self
            .detach_and_park_team_plans_transaction(team_id, now_ms)?
            .changed_plan_ids)
    }

    pub fn detach_and_park_team_plans_transaction(
        &self,
        team_id: &str,
        now_ms: u64,
    ) -> Result<TeamPlanDetach, PlanError> {
        let _guard = self.team_plan_lock.lock();
        self.detach_matching_team_plans(|bound| bound == team_id, now_ms)
    }

    pub fn rollback_team_plan_detach(&self, transaction: TeamPlanDetach) -> Result<(), PlanError> {
        let _guard = self.team_plan_lock.lock();
        for plan in transaction.original_plans {
            self.restore_team_plan(&plan)?;
        }
        Ok(())
    }

    pub fn detach_and_park_dangling_team_plans(
        &self,
        live: &std::collections::BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let _guard = self.team_plan_lock.lock();
        Ok(self
            .detach_matching_team_plans(|bound| !live.contains(bound), now_ms)?
            .changed_plan_ids)
    }

    fn detach_matching_team_plans<F>(
        &self,
        matches: F,
        now_ms: u64,
    ) -> Result<TeamPlanDetach, PlanError>
    where
        F: Fn(&str) -> bool,
    {
        let mut ids: Vec<_> = self
            .list_full()
            .into_iter()
            .filter_map(|plan| {
                plan.team_id
                    .as_deref()
                    .filter(|team| matches(team))
                    .map(|_| plan.id)
            })
            .collect();
        ids.sort();
        let original_plans: Vec<_> = ids.iter().filter_map(|id| self.get(id)).collect();
        for id in &ids {
            if let Err(error) = self.with_plan(id, now_ms, |plan| {
                plan.team_id = None;
                if plan.status == PlanStatus::Active
                    && !matches!(plan.mode, PlanMode::Paused | PlanMode::Complete)
                {
                    plan.paused_from_mode = Some(plan.mode.clone());
                    plan.mode = PlanMode::Paused;
                }
                Ok(())
            }) {
                for original in &original_plans {
                    let _ = self.restore_team_plan(original);
                }
                return Err(error);
            }
        }
        Ok(TeamPlanDetach {
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
        let mut plan = arc.lock();
        *plan = original.clone();
        let path = self
            .persist_plan_and_spec(&plan)
            .map_err(|error| PlanError::Io(error.to_string()))?;
        self.note_plan_write(&plan, &path);
        self.index
            .write()
            .insert(plan.id.clone(), Self::index_entry(&plan, path));
        Ok(())
    }
}
