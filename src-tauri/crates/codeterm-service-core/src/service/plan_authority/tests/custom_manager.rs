use crate::multiplexer::types::AgentRole;
use crate::plan::PlanningState;
use crate::service::plan::NewTask;
use crate::service::plan_authority::test_fixture::fixture;
use crate::service::plan_authority::{
    AuthenticatedPlanCaller, PlanAuthorityError, PlanOperationError,
};
use crate::service::test_fixture::ServiceFixture;

/// A live, attested custom manager in the plan's Team, parented to the plan owner.
fn custom_manager(
    fixture: &ServiceFixture,
    owner: &AuthenticatedPlanCaller,
) -> AuthenticatedPlanCaller {
    let owner_tab = owner.tab_id.clone().unwrap();
    let mut mux = fixture.service.mux.write();
    let workspace = mux.find_workspace_for_tab(&owner_tab).unwrap();
    let tab_id = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role_kind(
        &tab_id,
        AgentRole::Manager,
        Some("security_lead"),
        Some(&owner_tab),
    )
    .unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("custom-session".into());
    tab.session_binding_nonce = Some("custom-nonce".into());
    tab.reported_session_binding_nonce = Some("custom-nonce".into());
    AuthenticatedPlanCaller {
        tab_id: Some(tab_id),
        session_id: Some("custom-session".into()),
        binding_nonce: Some("custom-nonce".into()),
    }
}

#[test]
fn a_custom_manager_cannot_approve_draft_or_dispatch_a_plan() {
    let (fixture, plan_id, _, owner) = fixture();
    let manager = custom_manager(&fixture, &owner);
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Custom manager fixture".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    assert!(matches!(
        fixture
            .service
            .approve_and_activate_plan(&plan_id, &manager, None, 3),
        Err(PlanOperationError::Authority(
            PlanAuthorityError::RoleUnauthorized
        ))
    ));
    assert_eq!(
        fixture
            .service
            .plan_store
            .get(&plan_id)
            .unwrap()
            .planning_state,
        PlanningState::Drafting
    );
    assert!(matches!(
        fixture
            .service
            .authorize_draft_structure_mutation_typed(&plan_id, &manager),
        Err(PlanAuthorityError::RoleUnauthorized)
    ));

    fixture
        .service
        .approve_and_activate_plan(&plan_id, &owner, None, 4)
        .unwrap();
    assert!(matches!(
        fixture
            .service
            .authorize_execution_action(&plan_id, &manager),
        Err(PlanAuthorityError::RoleUnauthorized)
    ));
    assert!(matches!(
        fixture
            .service
            .authorize_task_execution_action(&task_id, &manager),
        Err(PlanAuthorityError::RoleUnauthorized)
    ));
    assert!(fixture
        .service
        .authorize_execution_action(&plan_id, &owner)
        .is_ok());
}
