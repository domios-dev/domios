use super::*;

pub struct PreparedLifecycleChanges {
    expected: Vec<Plan>,
    team_owners: Vec<(String, Option<(String, String)>)>,
    selector: PlanMatch,
    originals: Vec<Plan>,
    staged: Vec<Plan>,
}

impl PlanStore {
    fn lifecycle_candidates_checked(&self, selector: &PlanMatch) -> Result<Vec<Plan>, PlanError> {
        self.database
            .plan_ids_matching(selector)?
            .iter()
            .map(|id| self.try_get(id))
            .collect()
    }

    pub fn prepare_agent_lifecycle_with<T>(
        &self,
        now_ms: u64,
        prepare: impl FnOnce() -> Result<(Vec<AgentLifecyclePlanChange>, T), String>,
    ) -> Result<(PreparedLifecycleChanges, T), String> {
        let _writer = self.mutation_writer.enter();
        let (changes, payload) = prepare()?;
        let prepared = self
            .prepare_agent_lifecycle_changes(changes, now_ms)
            .map_err(|error| error.to_string())?;
        Ok((prepared, payload))
    }

    pub fn prepare_agent_lifecycle_changes(
        &self,
        changes: Vec<AgentLifecyclePlanChange>,
        now_ms: u64,
    ) -> Result<PreparedLifecycleChanges, PlanError> {
        let mut team_owners = Vec::new();
        for rebind in changes
            .iter()
            .filter_map(|change| change.plan_rebind.as_ref())
        {
            self.plan_identity_projection(&rebind.plan_id)?;
            team_owners.push((
                rebind.team_id.clone(),
                self.active_plan_for_team(&rebind.team_id),
            ));
        }
        let selector = PlanMatch::Any(changes.iter().map(lifecycle_candidates).collect());
        let expected = self.lifecycle_candidates_checked(&selector)?;
        let mut originals = Vec::new();
        let mut staged = Vec::new();
        for original in &expected {
            let mut candidate = original.clone();
            for change in &changes {
                apply_change(&mut candidate, change, now_ms);
            }
            if &candidate != original {
                candidate.updated_at = now_ms;
                tag_model::normalize_plan(&mut candidate);
                originals.push(original.clone());
                staged.push(candidate);
            }
        }
        Ok(PreparedLifecycleChanges {
            expected,
            team_owners,
            selector,
            originals,
            staged,
        })
    }

    pub fn commit_prepared_agent_lifecycle_changes(
        &self,
        prepared: PreparedLifecycleChanges,
    ) -> Result<Vec<String>, PlanError> {
        let writer = self.mutation_writer.enter();
        let current = self.lifecycle_candidates_checked(&prepared.selector)?;
        if prepared
            .team_owners
            .iter()
            .any(|(team, expected)| self.active_plan_for_team(team) != *expected)
            || current.len() != prepared.expected.len()
            || current
                .iter()
                .any(|plan| !prepared.expected.iter().any(|expected| expected == plan))
        {
            return Err(PlanError::Io(
                "lifecycle conflict: plan changed during preparation; retry by intent".into(),
            ));
        }
        self.commit_lifecycle_candidates(&writer, prepared.originals, prepared.staged)
            .map(|transaction| transaction.changed_plan_ids)
    }
}
