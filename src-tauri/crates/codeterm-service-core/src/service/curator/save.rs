use super::{authority::grant_matches, CuratorIdentity, ServiceContext};
use codeterm_blueprint_types::blueprint_memory::curator;
use std::collections::BTreeSet;

impl ServiceContext {
    pub fn curator_save_candidates(
        &self,
        plan_id: &str,
        now: u64,
    ) -> Result<std::collections::BTreeMap<String, BTreeSet<String>>, String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan absent")?;
        let ports = self
            .try_ext::<crate::ports::sentinel::SentinelPorts>()
            .ok_or("Blueprints unavailable")?;
        let ids = ports
            .store
            .records()
            .into_iter()
            .filter(|record| record.team_id == plan.team_id)
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let observed = ports.store.save_candidates();
        self.plan_store
            .with_plan(plan_id, now, |plan| {
                for task in plan
                    .tasks
                    .iter()
                    .filter(|task| task.status == crate::plan::TaskStatus::Done)
                {
                    for skill in task
                        .execution_attempts
                        .iter()
                        .filter_map(|attempt| attempt.report.as_ref())
                        .flat_map(|report| &report.loaded_plan_skills)
                    {
                        plan.control_loop
                            .save_candidates
                            .entry(format!("plan-skill:{}", skill.slug))
                            .or_default()
                            .insert(task.id.clone());
                    }
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
    pub fn curator_save_solution(
        &self,
        identity: &CuratorIdentity,
        candidate: &str,
        shape: &str,
        episodes: BTreeSet<String>,
        now: u64,
        save: impl FnOnce(&BTreeSet<String>, bool) -> Result<String, String>,
    ) -> Result<String, String> {
        self.plan_store
            .with_plan(&identity.plan_id, now, |plan| {
                let run = plan
                    .control_loop
                    .running
                    .as_mut()
                    .filter(|run| {
                        run.number == identity.number
                            && run.tab_id.as_deref() == Some(identity.tab_id.as_str())
                            && run.session_id.as_deref() == Some(identity.session_id.as_str())
                            && run.binding_nonce.as_deref() == Some(identity.binding_nonce.as_str())
                    })
                    .ok_or_else(|| {
                        crate::service::plan::PlanError::Validation("Curator round changed".into())
                    })?;
                if plan
                    .control_loop
                    .save_candidate_shapes
                    .get(candidate)
                    .is_some_and(|prior| prior != shape)
                {
                    plan.control_loop.save_candidates.remove(candidate);
                }
                plan.control_loop
                    .save_candidate_shapes
                    .insert(candidate.into(), shape.into());
                let evidence = plan
                    .control_loop
                    .save_candidates
                    .entry(candidate.into())
                    .or_default();
                evidence.extend(episodes);
                let granted = run.grant.as_ref().is_some_and(|grant| {
                    grant_matches(grant, identity)
                        && grant.save_candidates.iter().any(|id| id == candidate)
                });
                let result = curator::decide(evidence, granted, run.save_id.is_some(), &[])
                    .and_then(|_| save(evidence, granted));
                match result {
                    Ok(id) => {
                        run.save_id = Some(id.clone());
                        Ok(Ok(id))
                    }
                    Err(reason) => {
                        run.refuse_action(format!("save:{candidate}: {reason}"));
                        Ok(Err(reason))
                    }
                }
            })
            .map_err(|error| error.to_string())?
    }
}
