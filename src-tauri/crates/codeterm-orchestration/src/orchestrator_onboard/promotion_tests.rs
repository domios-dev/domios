use codeterm_service_core::multiplexer::state::MultiplexerState;

#[test]
fn host_promotion_of_a_teamless_orchestrator_mints_its_team() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("promotion-fixture".into());
        tab.agent_session_id = Some("promotion-session".into());
        tab.session_binding_nonce = Some("promotion-nonce".into());
        tab.reported_session_binding_nonce = Some("promotion-nonce".into());
    }
    mux.set_orchestrator_role(&tab_id).unwrap();
    assert!(mux.get_tab(&tab_id).unwrap().team_id.is_none());
    let fixture = crate::test_fixture::ServiceFixture::new(mux);

    crate::make_orchestrator(&fixture.service, &tab_id).unwrap();

    let mux = fixture.service.mux.read();
    let team_id = mux
        .get_tab(&tab_id)
        .unwrap()
        .team_id
        .clone()
        .expect("a successful promotion leaves the Orchestrator in a Team");
    assert!(mux.get_team(&team_id).is_some());
}
