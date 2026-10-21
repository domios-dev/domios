use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::mem::{store::MemUpdate, MemKind, NewMemItem, SearchQuery};
use codeterm_blueprint_types::blueprint_memory::curator::{self, Decision, Relation};
use codeterm_mem::manager_crud::SaveOutcome;

impl ServiceContext {
    pub fn save_memory_for_caller(
        &self,
        caller: &AuthenticatedPlanCaller,
        mut item: NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, String> {
        if !caller
            .tab_id
            .as_deref()
            .is_some_and(|tab| self.is_curator_tab(tab))
        {
            return self.mem.save_with_outcome(item, replace);
        }
        let identity = self.curator_identity(caller)?;
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
        let plan = self
            .plan_store
            .get(&identity.plan_id)
            .ok_or("plan absent")?;
        let episodes = plan
            .tasks
            .iter()
            .filter(|task| {
                task.status == crate::plan::TaskStatus::Done
                    && task
                        .execution_attempts
                        .iter()
                        .filter_map(|attempt| attempt.report.as_ref())
                        .any(|report| {
                            report
                                .loaded_plan_skills
                                .iter()
                                .any(|skill| skill.slug == slug)
                        })
            })
            .map(|task| task.id.clone())
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let shape = curator::fingerprint(&item.body);
        let id = self.curator_save_solution(
            &identity,
            &candidate,
            &shape,
            episodes,
            now,
            |episodes, granted| {
                let hits = self.mem.search(&SearchQuery {
                    query: item.title.clone(),
                    limit: Some(3),
                    ..Default::default()
                })?;
                let relations = hits
                    .iter()
                    .map(|hit| {
                        if hit.item.kind == "skill"
                            && hit.item.body.split("\n\nCurator evidence:").next()
                                == Some(item.body.as_str())
                        {
                            Relation::Covers(hit.item.id.clone())
                        } else {
                            Relation::Conflict(hit.item.id.clone())
                        }
                    })
                    .collect::<Vec<_>>();
                let decision = curator::decide(episodes, granted, false, &relations)?;
                if let Decision::Improve(id) = &decision {
                    if !replace {
                        return Err(format!("covered by {id}; use --replace to improve"));
                    }
                }
                let previous = match &decision {
                    Decision::Improve(id) => self.mem.get(id)?,
                    Decision::Save => None,
                };
                let round_prefix = format!("curator:round:{}:", identity.plan_id);
                let prior_round = previous.as_ref().and_then(|item| {
                    item.tags
                        .iter()
                        .find_map(|tag| tag.strip_prefix(&round_prefix)?.parse().ok())
                });
                let prior_confidence = previous
                    .as_ref()
                    .and_then(|item| {
                        item.tags
                            .iter()
                            .find_map(|tag| tag.strip_prefix("confidence:"))
                    })
                    .unwrap_or("low");
                item.tags.retain(|tag| {
                    !tag.starts_with("confidence:")
                        && !tag.starts_with("curator:round:")
                        && !tag.starts_with("curator:observed:")
                });
                item.tags.extend([
                    "curator".into(),
                    identity.plan_id.clone(),
                    format!(
                        "confidence:{}",
                        curator::confidence(prior_round, identity.number, prior_confidence)
                    ),
                    format!("{round_prefix}{}", identity.number),
                    format!("curator:observed:{}", now / 1000),
                ]);
                item.body.push_str(&format!(
                    "\n\nCurator evidence: {}{}",
                    episodes.iter().cloned().collect::<Vec<_>>().join(" "),
                    if granted {
                        format!(" grant:{}:{}", identity.plan_id, identity.number)
                    } else {
                        String::new()
                    }
                ));
                match decision {
                    Decision::Save => self.mem.save(item),
                    Decision::Improve(id) => {
                        self.mem.update(
                            &id,
                            MemUpdate {
                                body: Some(item.body),
                                tags: Some(item.tags),
                                ..Default::default()
                            },
                        )?;
                        Ok(id)
                    }
                }
            },
        )?;
        Ok(SaveOutcome {
            id,
            outcome: "saved".into(),
        })
    }
}
