use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

pub(crate) enum TeamPlanError {
    Team(String),
    Plan(PlanError),
}

impl ServiceContext {
    pub(crate) fn rename_team(
        &self,
        team_id: &str,
        name: String,
        caller_pane_id: Option<&str>,
    ) -> Result<Team, String> {
        let mut mux = self.mux.write();
        authorize_team_manager(&mux, team_id, caller_pane_id)?;
        let team = mux.rename_team(team_id, name)?;
        drop(mux);
        self.emit_state_changed();
        Ok(team)
    }

    pub(crate) fn adopt_plan_for_team(
        &self,
        team_id: &str,
        plan_id: &str,
        caller_pane_id: Option<&str>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, TeamPlanError> {
        if let Err(message) = authorize_team_manager(&self.mux.read(), team_id, caller_pane_id) {
            return Err(TeamPlanError::Team(message));
        }
        let plan = self
            .plan_store
            .bind_plan_to_team(plan_id, team_id, now_ms)
            .map_err(TeamPlanError::Plan)?;
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
        self.emit_state_changed();
        Ok(plan)
    }
}

fn authorize_team_manager(
    mux: &MultiplexerState,
    team_id: &str,
    caller_pane_id: Option<&str>,
) -> Result<(), String> {
    if mux.get_team(team_id).is_none() {
        return Err(format!("Team {team_id} not found"));
    }
    let Some(caller_pane_id) = caller_pane_id else {
        return Ok(());
    };
    let caller = mux
        .get_pane(caller_pane_id)
        .ok_or_else(|| format!("Caller pane {caller_pane_id} not found"))?;
    if caller.team_id.as_deref() != Some(team_id) {
        return Err(format!(
            "Caller pane {caller_pane_id} is not a member of Team {team_id}"
        ));
    }
    let manager = caller.role == Some(AgentRole::Orchestrator)
        || (caller.role == Some(AgentRole::Manager)
            && caller.role_profile.as_deref() == Some("planner"));
    if !manager {
        return Err(format!(
            "Caller pane {caller_pane_id} is not authorized to manage Team {team_id}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team_members() -> (MultiplexerState, String, String, String, String) {
        let (mut mux, orchestrator) = MultiplexerState::new();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let planner = mux.create_pane();
        mux.set_pane_role(&planner, AgentRole::Manager, Some(&orchestrator))
            .unwrap();
        mux.get_pane_mut(&planner).unwrap().role_profile = Some("planner".into());
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, planner, worker)
    }

    #[test]
    fn team_manager_permission_allows_orchestrator_and_planner() {
        let (mux, team_id, orchestrator, planner, _) = team_members();
        assert!(authorize_team_manager(&mux, &team_id, None).is_ok());
        assert!(authorize_team_manager(&mux, &team_id, Some(&orchestrator)).is_ok());
        assert!(authorize_team_manager(&mux, &team_id, Some(&planner)).is_ok());
    }

    #[test]
    fn mux_rename_team_rejects_worker_and_foreign_pane() {
        let (mut mux, team_id, _, _, worker) = team_members();
        assert!(authorize_team_manager(&mux, &team_id, Some(&worker)).is_err());
        let foreign = mux.create_pane();
        assert!(authorize_team_manager(&mux, &team_id, Some(&foreign)).is_err());
    }
}
