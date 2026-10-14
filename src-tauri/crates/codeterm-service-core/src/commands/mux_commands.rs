use crate::service::{ServiceState, TeamMutationCaller};
use codeterm_types::team::Team;
#[cfg(test)]
pub fn mux_rename_team_core(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    crate::service::rename_team_core(
        mux,
        &team_id,
        name,
        &TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
    )
}

pub(crate) fn mux_rename_team_service_core(
    svc: &crate::service::ServiceContext,
    team_id: String,
    name: String,
    caller_tab_id: Option<String>,
) -> Result<Team, String> {
    svc.rename_team(
        &team_id,
        name,
        TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
    )
}

pub(crate) async fn mux_add_team_member_service_core(
    svc: &ServiceState,
    team_id: String,
    provider: String,
    role: String,
    caller_tab_id: Option<String>,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
) -> Result<crate::service::AddTeamMemberResult, String> {
    // No caller is the bootstrap door: a human placing the first manager into a Team
    // whose members cannot spawn. The service refuses it for any non-anchoring role.
    svc.add_team_member(
        &team_id,
        caller_tab_id.as_deref(),
        crate::service::AddTeamMemberParams {
            provider,
            role,
            title,
            task,
            model,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use codeterm_events::SystemEvent;

    fn team_members() -> (MultiplexerState, String, String, String, String) {
        let (mut mux, orchestrator) = MultiplexerState::new();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
        let planner = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role_kind(
            &planner,
            AgentRole::Manager,
            Some("planner"),
            Some(&orchestrator),
        )
        .unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, planner, worker)
    }

    /// Renaming needs no spawner. The old gate demanded a caller tab, so a Team
    /// whose only manager could not spawn had no way to be renamed either — a
    /// requirement `rename_team_core` never imposed.
    #[test]
    fn mux_rename_team_accepts_a_human_caller() {
        let (mut mux, team_id, orchestrator, _planner, _worker) = team_members();
        let renamed = mux_rename_team_core(&mut mux, team_id.clone(), "Renamed".into(), None)
            .expect("a human rename carries no caller tab");
        assert_eq!(renamed.name.as_deref(), Some("Renamed"));
        let by_agent = mux_rename_team_core(
            &mut mux,
            team_id,
            "By the manager".into(),
            Some(orchestrator),
        )
        .expect("an agent caller still renames");
        assert_eq!(by_agent.name.as_deref(), Some("By the manager"));
    }

    #[test]
    fn mux_rename_team_core_allows_orchestrator_and_planner() {
        let (mut mux, team_id, orchestrator, planner, _) = team_members();

        let by_orchestrator = mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Orchestrated".into(),
            Some(orchestrator),
        )
        .unwrap();
        assert_eq!(by_orchestrator.name.as_deref(), Some("Orchestrated"));

        let by_planner =
            mux_rename_team_core(&mut mux, team_id.clone(), "Planned".into(), Some(planner))
                .unwrap();
        assert_eq!(by_planner.name.as_deref(), Some("Planned"));
        assert_eq!(
            mux.get_team(&team_id).unwrap().name.as_deref(),
            Some("Planned")
        );
    }

    #[test]
    fn mux_rename_team_core_rejects_worker_and_foreign_tab() {
        let (mut mux, team_id, _, _, worker) = team_members();
        let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
        let foreign = mux.create_tab(&workspace_id).unwrap();

        assert!(mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Denied worker".into(),
            Some(worker),
        )
        .is_err());
        assert!(mux_rename_team_core(
            &mut mux,
            team_id.clone(),
            "Denied foreign".into(),
            Some(foreign),
        )
        .is_err());
        assert_eq!(mux.get_team(&team_id).unwrap().name, None);
    }

    #[test]
    fn team_rename_emits_exactly_one_mux_state_changed() {
        let (mut mux, team_id, orchestrator, _, _) = team_members();
        let events = codeterm_events::EventBroadcaster::new();
        let mut receiver = events.subscribe();

        crate::service::rename_team_with_state_change(
            &mut mux,
            &events,
            &team_id,
            "Renamed".into(),
            &TeamMutationCaller::AgentTab(orchestrator.clone()),
        )
        .unwrap();
        assert_eq!(
            mux.get_team(&team_id).unwrap().name.as_deref(),
            Some("Renamed")
        );

        assert!(matches!(
            receiver.try_recv().unwrap().event,
            SystemEvent::MuxStateChanged
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
