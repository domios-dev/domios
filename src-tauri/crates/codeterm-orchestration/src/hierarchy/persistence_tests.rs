use crate::traits::HierarchyTeamPlanCreationExt;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::persistence_test_support::close;
use codeterm_service_core::service::TeamPlanCaller;
#[test]
fn plan_team_binding_round_trip_expected_red() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&tab_id).unwrap();
    let instance = crate::test_fixture::open_instance_with_mux(root.path(), mux);
    let outcome = instance
        .fixture
        .service
        .create_team_plan_for_caller(
            "round-trip plan".into(),
            root.path().to_string_lossy().into_owned(),
            codeterm_service_core::plan::OrchestratorRef {
                tab_id: Some(tab_id.clone()),
                session_id: None,
                title: Some("orchestrator".into()),
            },
            TeamPlanCaller {
                tab_id: None,
                session_id: None,
                binding_nonce: None,
            },
            Vec::new(),
            None,
            1_700_000_000_000,
        )
        .expect("create Team and bind its Plan through ServiceContext");
    let team_id = outcome.team_id;
    let plan_id = outcome.plan.id;

    let restarted = instance.restart(root.path());
    let plan = restarted
        .fixture
        .service
        .plan_store
        .get(&plan_id)
        .expect("plan database survives restart");
    assert_eq!(plan.team_id.as_deref(), Some(team_id.as_str()));
    assert!(
        restarted
            .fixture
            .service
            .mux
            .read()
            .get_team(&team_id)
            .is_some(),
        "the Plan's Team binding must also survive restart"
    );
    close(restarted);
}
