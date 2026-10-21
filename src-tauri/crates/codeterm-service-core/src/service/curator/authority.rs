use super::*;

#[derive(Debug, Clone)]
pub struct CuratorIdentity {
    pub plan_id: String,
    pub number: u64,
    pub tab_id: String,
    pub session_id: String,
    pub binding_nonce: String,
}

pub fn grant_matches(grant: &CuratorGrant, identity: &CuratorIdentity) -> bool {
    grant.round == identity.number
        && grant.tab_id == identity.tab_id
        && grant.session_id == identity.session_id
        && grant.binding_nonce == identity.binding_nonce
}

impl ServiceContext {
    pub fn is_curator_tab(&self, tab: &str) -> bool {
        self.mux
            .read()
            .get_tab(tab)
            .is_some_and(|tab| tab.role_profile.as_deref() == Some("curator"))
    }

    pub fn authorize_curator_operation(
        &self,
        tab_id: &str,
        id: Option<&str>,
        action: &str,
    ) -> Result<bool, String> {
        if !self.is_curator_tab(tab_id) {
            return Ok(false);
        }
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Curator tab absent")?;
        let caller = AuthenticatedPlanCaller {
            tab_id: Some(tab_id.into()),
            session_id: tab.agent_session_id,
            binding_nonce: tab.session_binding_nonce,
        };
        if matches!(action, "stop" | "delete" | "apply") {
            if let Some(id) = id {
                if self.authorize_curator_blueprint(&caller, id).is_ok() {
                    return Ok(true);
                }
            }
        }
        let identity = self.curator_identity(&caller)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.plan_store
            .with_plan(&identity.plan_id, now, |plan| {
                if let Some(run) = &mut plan.control_loop.running {
                    run.refused_actions
                        .push(format!("{action}:{}", id.unwrap_or("none")));
                    run.refused_actions.truncate(20);
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Err("Curator action requires a matching grant for this round".into())
    }

    pub fn curator_identity(
        &self,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<CuratorIdentity, String> {
        let tab_id = caller.tab_id.as_deref().ok_or("Curator tab required")?;
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Curator tab absent")?;
        let session = caller
            .session_id
            .as_deref()
            .ok_or("Curator session required")?;
        let nonce = caller
            .binding_nonce
            .as_deref()
            .ok_or("Curator binding required")?;
        if tab.agent_session_id.as_deref() != Some(session)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
            || tab.reported_session_binding_nonce.as_deref() != Some(nonce)
        {
            return Err("Curator session binding changed".into());
        }
        let identity = self
            .plan_store
            .list_full()
            .into_iter()
            .find_map(|plan| {
                let run = plan.control_loop.running.as_ref()?;
                (run.tab_id.as_deref() == Some(tab_id)
                    && run.session_id.as_deref().is_none_or(|s| s == session)
                    && run.binding_nonce.as_deref().is_none_or(|n| n == nonce))
                .then(|| CuratorIdentity {
                    plan_id: plan.id.clone(),
                    number: run.number,
                    tab_id: tab_id.into(),
                    session_id: session.into(),
                    binding_nonce: nonce.into(),
                })
            })
            .ok_or_else(|| "no active Curator round for this tab session".to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.plan_store
            .with_plan(&identity.plan_id, now, |plan| {
                let run = plan
                    .control_loop
                    .running
                    .as_mut()
                    .ok_or_else(|| crate::service::plan::PlanError::Io("round ended".into()))?;
                if run.number != identity.number
                    || run.session_id.as_deref().is_some_and(|s| s != session)
                    || run.binding_nonce.as_deref().is_some_and(|n| n != nonce)
                {
                    return Err(crate::service::plan::PlanError::Io(
                        "round session changed".into(),
                    ));
                }
                run.session_id = Some(session.into());
                run.binding_nonce = Some(nonce.into());
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(identity)
    }

    pub fn curator_grant(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        tab_id: &str,
        scope: super::CuratorGrantScope,
        now: u64,
    ) -> Result<CuratorGrant, String> {
        let super::CuratorGrantScope {
            blueprints,
            plan_skills,
            saves,
        } = scope;
        self.authorize_plan_orchestrator(plan_id, caller)?;
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Curator tab absent")?;
        let identity = self.curator_identity(&AuthenticatedPlanCaller {
            tab_id: Some(tab_id.into()),
            session_id: tab.agent_session_id,
            binding_nonce: tab.session_binding_nonce,
        })?;
        if identity.plan_id != plan_id {
            return Err("Curator belongs to another plan".into());
        }
        if blueprints.is_empty() && plan_skills.is_empty() && saves.is_empty() {
            return Err("grant needs a Blueprint, plan skill or save candidate".into());
        }
        self.plan_store
            .with_plan(plan_id, now, |plan| {
                let run = plan
                    .control_loop
                    .running
                    .as_mut()
                    .ok_or_else(|| crate::service::plan::PlanError::Io("round ended".into()))?;
                if blueprints
                    .iter()
                    .any(|id| !run.flagged_blueprints.contains(id))
                {
                    return Err(crate::service::plan::PlanError::Io(
                        "only daemon-flagged duplicate or stale Blueprints may be granted".into(),
                    ));
                }
                if plan_skills.iter().any(|slug| {
                    slug.is_empty()
                        || !slug
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                }) {
                    return Err(crate::service::plan::PlanError::Io(
                        "invalid plan skill slug".into(),
                    ));
                }
                let mut grant = run
                    .grant
                    .clone()
                    .filter(|g| grant_matches(g, &identity))
                    .unwrap_or(CuratorGrant {
                        tab_id: identity.tab_id,
                        session_id: identity.session_id,
                        binding_nonce: identity.binding_nonce,
                        round: identity.number,
                        ..Default::default()
                    });
                grant.blueprints.extend(blueprints);
                grant.blueprints.sort();
                grant.blueprints.dedup();
                grant.plan_skills.extend(plan_skills);
                grant.plan_skills.sort();
                grant.plan_skills.dedup();
                if saves
                    .iter()
                    .any(|id| id.trim().is_empty() || id.len() > 200)
                {
                    return Err(crate::service::plan::PlanError::Io(
                        "invalid save candidate".into(),
                    ));
                }
                grant.save_candidates.extend(saves);
                grant.save_candidates.sort();
                grant.save_candidates.dedup();
                if grant.save_candidates.len() > 1 {
                    return Err(crate::service::plan::PlanError::Io(
                        "one save candidate per round".into(),
                    ));
                }
                if grant.plan_skills.len() > 1 {
                    return Err(crate::service::plan::PlanError::Io(
                        "a Curator round may touch one plan skill".into(),
                    ));
                }
                run.grant = Some(grant.clone());
                Ok(grant)
            })
            .map_err(|e| e.to_string())
    }

    pub fn authorize_curator_blueprint(
        &self,
        caller: &AuthenticatedPlanCaller,
        id: &str,
    ) -> Result<(), String> {
        let identity = self.curator_identity(caller)?;
        let plan = self
            .plan_store
            .get(&identity.plan_id)
            .ok_or("plan absent")?;
        let grant = plan
            .control_loop
            .running
            .and_then(|r| r.grant)
            .ok_or("Curator grant required")?;
        if grant_matches(&grant, &identity) && grant.blueprints.iter().any(|allowed| allowed == id)
        {
            Ok(())
        } else {
            Err("Blueprint is outside this round's grant".into())
        }
    }

    pub fn authorize_curator_plan_skill(
        &self,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        slug: &str,
    ) -> Result<(), String> {
        let identity = self.curator_identity(caller)?;
        if identity.plan_id != plan_id {
            return Err("Curator plan skill belongs to another plan".into());
        }
        let plan = self.plan_store.get(plan_id).ok_or("plan absent")?;
        let grant = plan
            .control_loop
            .running
            .and_then(|r| r.grant)
            .ok_or("Curator plan-skill grant required")?;
        if grant_matches(&grant, &identity) && grant.plan_skills.iter().any(|s| s == slug) {
            Ok(())
        } else {
            Err("plan skill outside this round's grant".into())
        }
    }
}
