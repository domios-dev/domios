use super::*;
use crate::multiplexer::state::LifecyclePatch;

impl ServiceContext {
    pub fn transition_agent_prepared_at(
        &self,
        now_ms: u64,
        request: impl FnOnce(
            &MultiplexerState,
            &crate::service::plan::PlanStore,
        ) -> Result<AgentTransitionRequest, String>,
        failure: TransitionFailurePoint,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let history = crate::multiplexer::state::ClosedTabsStore::in_memory();
        let mut before = self
            .mux
            .read()
            .begin_team_close_transaction_with_store(history);
        let (plans, (prepared, changed_bindings)) =
            self.plan_store.prepare_agent_lifecycle_with(now_ms, || {
                let mux = before.state_mut();
                let request = request(mux, &self.plan_store)?;
                refuse_unverified_task_bound_worker_transfer(mux, &self.plan_store, &request)?;
                let bindings_before = role_team_bindings(mux);
                let mut prepared = prepare_transition(mux, &self.plan_store, &request)?;
                if failure == TransitionFailurePoint::SkipPlanRebind {
                    return Err("injected lifecycle rebind omission".into());
                }
                let changed_bindings = role_team_bindings(prepared.staged_mux.state_mut())
                    .into_iter()
                    .filter_map(|(id, binding)| {
                        (bindings_before.get(&id) != Some(&binding)).then_some(id)
                    })
                    .collect::<Vec<_>>();
                let changes = std::mem::take(&mut prepared.plan_changes);
                Ok((changes, (prepared, changed_bindings)))
            })?;
        let patch = LifecyclePatch::new(before, prepared.staged_mux);
        patch.apply(&mut self.mux.write())?;
        let committed = if failure == TransitionFailurePoint::AfterPlanCommit {
            Err("injected agent transition failure".to_string())
        } else {
            self.plan_store
                .commit_prepared_agent_lifecycle_changes(plans)
                .map_err(|error| error.to_string())
        };
        match committed {
            Ok(changed) => Ok((changed, changed_bindings)),
            Err(error) => {
                patch.revert(&mut self.mux.write()).map_err(|rollback| {
                    format!("{error}; lifecycle rollback conflict: {rollback}")
                })?;
                Err(error)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn plan_owner_request(
    owner: crate::plan::OrchestratorRef,
    plan_id: &str,
    team_id: &str,
    reason: &str,
    activate: bool,
    planner_replacement: Option<(String, Option<PlannerRef>)>,
    clear_planner: bool,
) -> Result<AgentTransitionRequest, String> {
    let tab_id = owner
        .tab_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or("plan rebind requires a destination Orchestrator tab")?;
    Ok(AgentTransitionRequest {
        tab_id,
        destination: AgentPlacement::orchestrator(Some(true)),
        plan_binding: None,
        require_verified_session: false,
        planner_replacement,
        plan_rebind: Some(AgentLifecyclePlanRebind {
            plan_id: plan_id.into(),
            owner,
            team_id: team_id.into(),
            reason: reason.into(),
            activate,
            clear_planner,
        }),
    })
}
