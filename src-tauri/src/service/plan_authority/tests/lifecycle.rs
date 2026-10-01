use super::*;

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovered_moved_owner_passes_lifecycle_guard_and_foreign_callers_fail() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::test_fixture::ServiceFixture;
    use std::sync::Arc;

    let (mut mux, old_owner) = MultiplexerState::new();
    let source = mux.find_workspace_for_tab(&old_owner).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&old_owner)
        .unwrap()
        .team_id;
    let owner = mux.create_tab(&source).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    mux.get_tab_mut(&owner).unwrap().team_id = Some(team.clone());
    let foreign = mux.create_tab(&source).unwrap();
    mux.set_orchestrator_role(&foreign).unwrap();
    mux.resolve_or_mint_team_for_parent(&foreign).unwrap();
    for (id, session, nonce) in [
        (&owner, "owner-session", "owner-nonce"),
        (&foreign, "foreign-session", "foreign-nonce"),
    ] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
    }
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    let mut plan = Plan::new(
        "Paused recovery".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&old_owner),
    );
    plan.team_id = Some(team.clone());
    svc.plan_store.save(&mut plan, 1).unwrap();
    svc.plan_store
        .pause_plan(&plan.id, "owner decision", Default::default(), 2)
        .unwrap();
    {
        let mut mux = svc.mux.write();
        mux.close_tab(&old_owner).unwrap();
        crate::service::agent_transition::recover_plan_ownership_for_new_orchestrator(
            &mut mux,
            &svc.plan_store,
            &plan.id,
            OrchestratorRef::from_tab(&owner),
            &team,
            "supported recovery",
            3,
        )
        .unwrap();
        let destination = mux.create_workspace("Moved".into(), None);
        mux.move_tab(&owner, &source, &destination.id).unwrap();
    }
    let before = svc.plan_store.get(&plan.id).unwrap();
    let owner_caller = AuthenticatedPlanCaller {
        tab_id: Some(owner.clone()),
        session_id: None,
        binding_nonce: Some("owner-nonce".into()),
    };
    assert_eq!(
        svc.classify_planning_caller(&plan.id, &owner_caller),
        Ok(PlanningAuthor::User)
    );
    for (caller, expected) in [
        (
            AuthenticatedPlanCaller {
                tab_id: None,
                ..owner_caller.clone()
            },
            PlanAuthorityError::BindingForeign,
        ),
        (
            AuthenticatedPlanCaller {
                tab_id: Some(foreign),
                binding_nonce: Some("foreign-nonce".into()),
                session_id: None,
            },
            PlanAuthorityError::RoleUnauthorized,
        ),
        (
            AuthenticatedPlanCaller {
                binding_nonce: Some("foreign-nonce".into()),
                ..owner_caller
            },
            PlanAuthorityError::BindingForeign,
        ),
    ] {
        assert_eq!(
            svc.classify_planning_caller(&plan.id, &caller),
            Err(expected)
        );
    }
    assert_eq!(svc.plan_store.get(&plan.id).unwrap(), before);
}
