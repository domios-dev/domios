use super::ServiceContext;
use std::collections::BTreeSet;

impl CuratorSaveExt for ServiceContext {
    fn curator_save_candidates(
        &self,
        plan_id: &str,
        team_id: &Option<String>,
        now: u64,
    ) -> Result<std::collections::BTreeMap<String, BTreeSet<String>>, String> {
        let ports = self
            .try_ext::<codeterm_service_core::ports::sentinel::SentinelPorts>()
            .ok_or("Blueprints unavailable")?;
        let ids = ports
            .store
            .records()
            .into_iter()
            .filter(|record| &record.team_id == team_id)
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let observed = ports.store.save_candidates();
        self.plan_store
            .edit_curator_save(plan_id, now, |plan| {
                retain_live_blueprints(&mut plan.control_loop, &ids);
                for (key, episodes) in &plan.completed_skill_episodes {
                    plan.control_loop
                        .save_candidates
                        .entry(key.clone())
                        .or_default()
                        .extend(episodes.iter().cloned());
                }
                for (id, candidate) in observed.into_iter().filter(|(id, _)| ids.contains(id)) {
                    if plan
                        .control_loop
                        .save_candidate_shapes
                        .get(&id)
                        .is_some_and(|shape| shape != &candidate.shape)
                    {
                        plan.control_loop.save_candidates.remove(&id);
                    }
                    plan.control_loop
                        .save_candidate_shapes
                        .insert(id.clone(), candidate.shape);
                    plan.control_loop
                        .save_candidates
                        .entry(id)
                        .or_default()
                        .extend(candidate.episodes);
                }
                Ok(plan.control_loop.save_candidates.clone())
            })
            .map_err(|error| error.to_string())
    }
}

fn retain_live_blueprints(state: &mut codeterm_types::plan::ControlLoop, live: &BTreeSet<String>) {
    state
        .save_candidates
        .retain(|id, _| id.starts_with("plan-skill:") || live.contains(id));
    state
        .save_candidate_shapes
        .retain(|id, _| id.starts_with("plan-skill:") || live.contains(id));
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;

pub trait CuratorSaveExt {
    fn curator_save_candidates(
        &self,
        plan_id: &str,
        team_id: &Option<String>,
        now: u64,
    ) -> Result<std::collections::BTreeMap<String, BTreeSet<String>>, String>;
}
