use super::AuthenticatedPlanCaller;
use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::test_fixture::ServiceFixture;

pub fn fixture() -> (
    ServiceFixture,
    String,
    AuthenticatedPlanCaller,
    AuthenticatedPlanCaller,
) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let planner = mux.create_tab(&workspace).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    for (tab_id, session, nonce) in [
        (&owner, "owner-session", "owner-nonce"),
        (&planner, "planner-session", "planner-nonce"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        // Make the tab pass `tab_has_live_agent` so the stale-after-relaunch
        // proof (which requires a genuinely live tab) is exercised.
        tab.agent_type = Some("provider-neutral".into());
        tab.chat_view = true;
        tab.pty_id = Some(format!("pty-{tab_id}"));
    }
    {
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.parent_tab_id = Some(owner.clone());
        tab.team_id = Some(team_id.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Draft graph".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let caller = |tab_id: String, session: &str, nonce: &str| AuthenticatedPlanCaller {
        tab_id: Some(tab_id),
        session_id: Some(session.into()),
        binding_nonce: Some(nonce.into()),
    };
    (
        fixture,
        plan.id,
        caller(planner, "planner-session", "planner-nonce"),
        caller(owner, "owner-session", "owner-nonce"),
    )
}
