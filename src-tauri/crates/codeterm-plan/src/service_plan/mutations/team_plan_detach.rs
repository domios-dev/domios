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
        let _writer_turn = self.mutation_writer.enter();
        let target_team_id = team_id.trim().to_string();
        self.detach_matching_team_plans(|bound| bound == target_team_id, now_ms)
    }

    pub fn rollback_team_plan_detach(&self, transaction: TeamPlanDetach) -> Result<(), PlanError> {
        let _writer_turn = self.mutation_writer.enter();
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
        let _writer_turn = self.mutation_writer.enter();
        Ok(self
            .detach_matching_team_plans(|team_id| !live.contains(team_id), now_ms)?
            .changed_plan_ids)
    }

    pub fn recover_placeholder_team_plans(
        &self,
        live: &std::collections::BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let plans = self.list_full();
        let mut repairs = Vec::new();
        for placeholder in plans.iter().filter(|plan| {
            plan.status == PlanStatus::Active
                && plan.tasks.is_empty()
                && plan
                    .team_id
                    .as_ref()
                    .is_some_and(|team| live.contains(team))
        }) {
            let placeholder_project = if placeholder.canonical_project_path.trim().is_empty() {
                &placeholder.project_path
            } else {
                &placeholder.canonical_project_path
            };
            let matches: Vec<_> = plans
                .iter()
                .filter(|plan| {
                    let project = if plan.canonical_project_path.trim().is_empty() {
                        &plan.project_path
                    } else {
                        &plan.canonical_project_path
                    };
                    plan.status == PlanStatus::Active
                        && plan.team_id.is_none()
                        && !plan.tasks.is_empty()
                        && plan.title.trim() == placeholder.title.trim()
                        && project == placeholder_project
                })
                .collect();
            if let [legacy] = matches.as_slice() {
                repairs.push((placeholder.clone(), (*legacy).clone()));
            }
        }

        let candidate_use: std::collections::HashMap<String, usize> = repairs.iter().fold(
            std::collections::HashMap::new(),
            |mut counts, (_, legacy)| {
                *counts.entry(legacy.id.clone()).or_default() += 1;
                counts
            },
        );
        repairs.retain(|(_, legacy)| candidate_use.get(&legacy.id) == Some(&1));
        let originals: Vec<_> = repairs
            .iter()
            .flat_map(|(placeholder, legacy)| [placeholder.clone(), legacy.clone()])
            .collect();
        let mut changed = Vec::new();
        for (placeholder, legacy) in repairs {
            let team_id = placeholder.team_id.clone().expect("placeholder has a team");
            let repair = self
                .with_plan_in_writer(&placeholder.id, now_ms, |plan| {
                    plan.team_id = None;
                    if !matches!(plan.mode, PlanMode::Paused | PlanMode::Complete) {
                        plan.paused_from_mode = Some(plan.mode.clone());
                        plan.mode = PlanMode::Paused;
                    }
                    Ok(())
                })
                .and_then(|_| {
                    self.with_plan_in_writer(&legacy.id, now_ms, |plan| {
                        plan.team_id = Some(team_id);
                        Ok(())
                    })
                });
            if let Err(error) = repair {
                for original in &originals {
                    let _ = self.restore_team_plan(original);
                }
                return Err(error);
            }
            changed.extend([placeholder.id, legacy.id]);
        }
        Ok(changed)
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
                let bound_team = plan.team_id.as_deref();
                bound_team.filter(|team| matches(team)).map(|_| plan.id)
            })
            .collect();
        ids.sort();
        let original_plans: Vec<_> = ids.iter().filter_map(|id| self.get(id)).collect();
        for id in &ids {
            if let Err(error) = self.with_plan_in_writer(id, now_ms, |plan| {
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
