use super::*;
use crate::multiplexer::state::{LifecyclePatch, LIFECYCLE_CONTEXT_CONFLICT};

pub(super) const PREPARE_ATTEMPTS: usize = 3;

impl ServiceContext {
    pub fn transition_agent_prepared_at(
        &self,
        now_ms: u64,
        mut request: impl FnMut(
            &MultiplexerState,
            &crate::service::plan::PlanStore,
        ) -> Result<AgentTransitionRequest, String>,
        failure: TransitionFailurePoint,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        self.transition_agent_prepared_with_at(
            now_ms,
            |mux, store| request(mux, store).map(|request| (Some(request), ())),
            failure,
        )
        .map(|(changed, bindings, ())| (changed, bindings))
    }

    pub fn transition_agent_prepared_with_at<T>(
        &self,
        now_ms: u64,
        mut prepare: impl FnMut(
            &mut MultiplexerState,
            &crate::service::plan::PlanStore,
        ) -> Result<(Option<AgentTransitionRequest>, T), String>,
        failure: TransitionFailurePoint,
    ) -> Result<(Vec<String>, Vec<String>, T), String> {
        let mut attempt = 1;
        loop {
            match self.transition_agent_prepared_once(now_ms, &mut prepare, failure) {
                Err(error) if error == LIFECYCLE_CONTEXT_CONFLICT && attempt < PREPARE_ATTEMPTS => {
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }

    /// Preparation reads a mux snapshot outside the guard, so a concurrent
    /// launch binding can invalidate it; the caller re-prepares from intent.
    fn transition_agent_prepared_once<T>(
        &self,
        now_ms: u64,
        prepare: &mut impl FnMut(
            &mut MultiplexerState,
            &crate::service::plan::PlanStore,
        ) -> Result<(Option<AgentTransitionRequest>, T), String>,
        failure: TransitionFailurePoint,
    ) -> Result<(Vec<String>, Vec<String>, T), String> {
        let history = crate::multiplexer::state::ClosedTabsStore::in_memory();
        let mut before = self
            .mux
            .read()
            .begin_team_close_transaction_with_store(history);
        let history = crate::multiplexer::state::ClosedTabsStore::in_memory();
        let mut staged = before
            .state_mut()
            .begin_team_close_transaction_with_store(history);
        let (plans, (staged, changed_bindings, payload)) =
            self.plan_store.prepare_agent_lifecycle_with(now_ms, || {
                let bindings_before = role_team_bindings(before.state_mut());
                let (request, payload) = prepare(staged.state_mut(), &self.plan_store)?;
                let (changes, mut staged) = if let Some(request) = request {
                    refuse_unverified_task_bound_worker_transfer(
                        staged.state_mut(),
                        &self.plan_store,
                        &request,
                    )?;
                    let mut prepared =
                        prepare_transition(staged.state_mut(), &self.plan_store, &request)?;
                    (
                        std::mem::take(&mut prepared.plan_changes),
                        prepared.staged_mux,
                    )
                } else {
                    (vec![], staged)
                };
                if failure == TransitionFailurePoint::SkipPlanRebind {
                    return Err("injected lifecycle rebind omission".into());
                }
                let changed_bindings = role_team_bindings(staged.state_mut())
                    .into_iter()
                    .filter_map(|(id, binding)| {
                        (bindings_before.get(&id) != Some(&binding)).then_some(id)
                    })
                    .collect::<Vec<_>>();
                Ok((changes, (staged, changed_bindings, payload)))
            })?;
        let patch = LifecyclePatch::new(before, staged);
        patch.apply(&mut self.mux.write())?;
        let committed = if failure == TransitionFailurePoint::AfterPlanCommit {
            Err("injected agent transition failure".to_string())
        } else {
            self.plan_store
                .commit_prepared_agent_lifecycle_changes(plans)
                .map_err(|error| error.to_string())
        };
        match committed {
            Ok(changed) => Ok((changed, changed_bindings, payload)),
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

impl ServiceContext {
    pub fn resolve_planner_parent_for_spawn(&self, plan_id: &str) -> Result<String, String> {
        self.resolve_planner_parent_using(plan_id, || plan_owner(&self.plan_store, plan_id))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_planner_parent_for_spawn_observed(
        &self,
        plan_id: &str,
        entered: impl FnOnce(),
    ) -> Result<String, String> {
        self.resolve_planner_parent_using(plan_id, || {
            entered();
            plan_owner(&self.plan_store, plan_id)
        })
    }

    fn resolve_planner_parent_using(
        &self,
        plan_id: &str,
        lookup: impl FnOnce() -> Result<String, String>,
    ) -> Result<String, String> {
        let owner = lookup()?;
        if self.mux.read().get_tab(&owner).is_none() {
            return Err(format!(
                "Plan {plan_id} owner tab {owner} is not live; pass --parent-tab explicitly"
            ));
        }
        Ok(owner)
    }
}
