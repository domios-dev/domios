use super::*;

#[test]
fn curator_input_refuses_users_and_foreign_agents_but_accepts_its_orchestrator() {
    assert_eq!(
        authorize(true, None, Some("owner")),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(
        authorize(true, Some("foreign"), Some("owner")),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(authorize(true, Some("owner"), Some("owner")), Ok(()));
    assert_eq!(
        authorize(true, Some("owner"), None),
        Err(TabInputError::CuratorReadOnly)
    );
    assert_eq!(authorize(false, None, None), Ok(()));
}

#[test]
fn curator_user_entry_points_refuse_before_pty_or_ledger_work() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().role_profile = Some("curator".into());
    mux.link_pty(&id, "round-pty".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let error = "curator_read_only: Curator round, read-only";
    assert_eq!(
        service.require_managed_send_allowed(&id, None).unwrap_err(),
        error
    );
    assert_eq!(
        service
            .require_managed_terminal_input_allowed("round-pty", b"a")
            .unwrap_err(),
        error
    );
    assert_eq!(
        service
            .require_managed_terminal_input_allowed("round-pty", b"\x1b")
            .unwrap_err(),
        error
    );
    assert!(service.require_admitted_input_allowed(&id, None).is_err());
    assert!(service
        .require_admitted_input_allowed(&id, Some("unknown"))
        .is_err());
    let admitted = service
        .attach_system_send(&id, "opaque host envelope")
        .unwrap();
    assert!(service
        .require_admitted_input_allowed(&id, Some(&admitted.send_id))
        .is_ok());
    assert_eq!(
        service
            .start_chat_send(&id, "opaque".into(), None, None)
            .unwrap_err(),
        error
    );
}

#[test]
fn curator_input_resolves_the_current_plan_owner_instead_of_any_sender() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.get_active_workspace().unwrap().id.clone();
    let curator = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&curator).unwrap().role_profile = Some("curator".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    service
        .set_ext(super::super::curator::CuratorTabs::default())
        .ok()
        .unwrap();
    let mut plan = Plan::new(
        "Fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    service.plan_store.save(&mut plan, 1).unwrap();
    service
        .ext::<super::super::curator::CuratorTabs>()
        .0
        .write()
        .insert(curator.clone(), plan.id.clone());
    assert!(service
        .require_managed_send_allowed(&curator, Some(&owner))
        .is_ok());
    assert!(service
        .require_managed_send_allowed(&curator, None)
        .unwrap_err()
        .starts_with("curator_read_only:"));
}
