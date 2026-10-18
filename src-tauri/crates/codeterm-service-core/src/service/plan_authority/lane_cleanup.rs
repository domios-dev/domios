use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::multiplexer::types::Tab;
use crate::plan::Plan;
use crate::service::plan_reconcile::tab_has_live_agent;

fn owned_utility_can_clean(
    plan: &Plan,
    caller: &AuthenticatedPlanCaller,
    utility: Option<&Tab>,
    owner: Option<&Tab>,
) -> bool {
    let (Some(utility), Some(owner), Some(team), Some(nonce)) = (
        utility,
        owner,
        plan.team_id.as_deref(),
        caller
            .binding_nonce
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) else {
        return false;
    };
    caller.tab_id.as_deref() == Some(utility.id.as_str())
        && caller.session_id.is_none()
        && utility.session_binding_nonce.as_deref() == Some(nonce)
        && utility.is_utility_shell()
        && utility.agent_session_id.is_none()
        && utility.role_profile.is_none()
        && utility.task_binding.is_none()
        && utility.pty_id.is_some()
        && utility.exit_suspected_at.is_none()
        && utility.team_id.as_deref() == Some(team)
        && utility.parent_tab_id.as_deref() == Some(owner.id.as_str())
        && utility
            .close_restore
            .as_ref()
            .and_then(|state| state.utility_owner_tab_id.as_deref())
            == Some(owner.id.as_str())
        && plan.owner.tab_id.as_deref() == Some(owner.id.as_str())
        && plan.owner.session_id.is_some()
        && plan.owner.session_id == owner.agent_session_id
        && owner.team_id.as_deref() == Some(team)
        && owner.has_team_orchestrator_authority()
        && codeterm_types::agent::role_kind::plan_approval(
            owner.role,
            owner.role_profile.as_deref(),
        )
        && tab_has_live_agent(owner)
}

impl ServiceContext {
    pub fn authorize_plan_lane_cleanup(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<(), String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let utility_authorized = {
            let mux = self.mux.read();
            owned_utility_can_clean(
                &plan,
                caller,
                caller.tab_id.as_deref().and_then(|id| mux.get_tab(id)),
                plan.owner.tab_id.as_deref().and_then(|id| mux.get_tab(id)),
            )
        };
        if utility_authorized {
            log::debug!(
                "[plan.lanes_clean] authorized owned utility {:?} for {plan_id}",
                caller.tab_id
            );
            return Ok(());
        }
        self.authorize_plan_orchestrator(plan_id, caller)
    }
}

#[cfg(test)]
mod tests;
