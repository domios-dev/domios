use codeterm_service_core::service::startup_lease::pair_fixture::PairFixture;

#[test]
fn normal_team_close_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = codeterm_service_core::service::plan::PlanStore::new_for_test(dir.path());
    let events = codeterm_events::EventBroadcaster::new();
    let mut fixture = PairFixture::new();
    fixture
        .mux
        .set_orchestrator_role(&fixture.orchestrator)
        .unwrap();
    let team_id = fixture.team_id.clone();

    let (closed, _cleanup, _) = crate::close_team_with_state_change(
        &mut fixture.mux,
        &events,
        &plan_store,
        &team_id,
        &codeterm_service_core::service::TeamMutationCaller::AgentTab(fixture.orchestrator.clone()),
        1,
    )
    .expect("an ordinary team close still cascades a forming, leased Team");

    assert_eq!(closed.id, team_id);
    assert!(fixture.mux.get_team(&team_id).is_none());
    assert!(fixture.mux.get_tab(&fixture.orchestrator).is_none());
    assert!(fixture.mux.get_tab(&fixture.planner).is_none());
}
