use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::mem::{MemKind, NewMemItem};
use codeterm_blueprint_types::blueprint_memory::curator::{self, Decision};
use codeterm_mem::manager_crud::SaveOutcome;

impl ServiceContext {
    pub fn save_memory_for_caller(
        &self,
        caller: &AuthenticatedPlanCaller,
        item: NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, String> {
        if !caller
            .tab_id
            .as_deref()
            .is_some_and(|tab| self.is_curator_tab(tab))
        {
            return self.mem.save_with_outcome(item, replace);
        }
        let identity = self.authenticate_curator(caller)?;
        if [&item.title, &item.body]
            .into_iter()
            .chain(item.summary.iter())
            .chain(item.tags.iter())
            .any(|text| codeterm_blueprint_types::blueprint_memory::contains_credentials(text))
        {
            return Err("Curator solution contains credentials".into());
        }
        if item.kind != MemKind::Skill {
            return Err("Curator global saves must be generalised skills".into());
        }
        let candidate = item
            .tags
            .iter()
            .find_map(|tag| tag.strip_prefix("curator:candidate:"))
            .ok_or("Curator save needs curator:candidate:plan-skill:<slug> tag")?
            .to_owned();
        let slug = candidate
            .strip_prefix("plan-skill:")
            .ok_or("non-graph candidate must name a plan skill")?;
        let episodes = self
            .plan_store
            .curator_skill_episodes_projection(&identity.plan_id, slug)
            .map_err(|error| match error {
                crate::service::plan::PlanError::NotFound => "plan absent".to_string(),
                error => error.to_string(),
            })?;
        let now = self.curator_now()?;
        let shape = curator::fingerprint(&item.body);
        let id = self.curator_save_solution(
            &identity,
            &candidate,
            &shape,
            episodes.into_iter().collect(),
            now,
            |episodes, granted| {
                let relations = self.mem.curator_skill_relations(&item)?;
                let decision = curator::decide(episodes, granted, false, &relations)?;
                if let Decision::Improve(id) = &decision {
                    if !replace {
                        return Err(format!("covered by {id}; use --replace to improve"));
                    }
                }
                let improve = match &decision {
                    Decision::Improve(id) => Some(id.as_str()),
                    Decision::Save => None,
                };
                self.mem.save_curator_skill(
                    item,
                    improve,
                    codeterm_blueprint_types::blueprint_memory::CuratorEvidence {
                        plan: identity.plan_id.clone(),
                        round: identity.number,
                        episodes: episodes.iter().cloned().collect(),
                        granted,
                    },
                    (now / 1000) as i64,
                )
            },
        )?;
        Ok(SaveOutcome {
            id,
            outcome: "saved".into(),
        })
    }
}
