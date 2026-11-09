use super::*;
use crate::service_plan::store_db::hot_write::lifecycle_rows::{self, LifecyclePatch, TeamOwners};

pub struct PreparedLifecycleChanges {
    selector: PlanMatch,
    expected: Vec<(String, i64)>,
    team_owners: TeamOwners,
    patches: Vec<LifecyclePatch>,
}

impl PlanStore {
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
        for rebind in changes
            .iter()
            .filter_map(|change| change.plan_rebind.as_ref())
        {
            self.plan_identity_projection(&rebind.plan_id)?;
        }
        let selector = PlanMatch::Any(changes.iter().map(lifecycle_candidates).collect());
        let (plans, team_owners) = self.database.observe_lifecycle_rows(&selector, &changes)?;
        let expected = lifecycle_rows::expected(&plans);
        let mut patches = Vec::new();
        for original in plans {
            let mut candidate = original.clone();
            for change in &changes {
                apply_change(&mut candidate, change, now_ms);
            }
            if candidate != original {
                if let Some(patch) = lifecycle_rows::patch(original, candidate, now_ms) {
                    patches.push(patch);
                }
            }
        }
        Ok(PreparedLifecycleChanges {
            selector,
            expected,
            team_owners,
            patches,
        })
    }

    pub fn prepare_team_plan_unarchive(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<(PreparedLifecycleChanges, String), PlanError> {
        let _writer = self.mutation_writer.enter();
        let selector = PlanMatch::Ids(vec![plan_id.into()]);
        let (mut plans, _) = self.database.observe_lifecycle_rows(&selector, &[])?;
        let original = plans.pop().ok_or(PlanError::NotFound)?;
        let team = original
            .team_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            })?;
        let active = self.active_plan_for_team(&team);
        if let Some((existing_plan_id, existing_title)) = &active {
            if existing_plan_id != plan_id {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team,
                    existing_plan_id: existing_plan_id.clone(),
                    existing_title: existing_title.clone(),
                });
            }
        }
        let expected = vec![(original.id.clone(), original.revision)];
        let mut staged = original.clone();
        staged.status = crate::plan::PlanStatus::Active;
        staged.updated_at = now_ms;
        Ok((
            PreparedLifecycleChanges {
                selector,
                expected,
                team_owners: vec![(team.clone(), active)],
                patches: lifecycle_rows::patch(original, staged, now_ms)
                    .into_iter()
                    .collect(),
            },
            team,
        ))
    }

    pub fn commit_prepared_agent_lifecycle_changes(
        &self,
        prepared: PreparedLifecycleChanges,
    ) -> Result<Vec<String>, PlanError> {
        self.commit_prepared_lifecycle_transaction(prepared)
            .map(|transaction| transaction.changed_plan_ids)
    }

    pub(super) fn commit_prepared_lifecycle_transaction(
        &self,
        prepared: PreparedLifecycleChanges,
    ) -> Result<AgentLifecyclePlanTransaction, PlanError> {
        let turn = self.mutation_writer.enter();
        let (undo, receipts) = self.database.commit_lifecycle_rows(
            &prepared.selector,
            &prepared.expected,
            &prepared.team_owners,
            prepared.patches,
        )?;
        let changed_plan_ids = undo
            .patches
            .iter()
            .map(|patch| patch.before.id.clone())
            .collect();
        for receipt in receipts {
            self.publish_hot_receipt(&turn, receipt);
        }
        Ok(AgentLifecyclePlanTransaction {
            undo,
            changed_plan_ids,
        })
    }
}
