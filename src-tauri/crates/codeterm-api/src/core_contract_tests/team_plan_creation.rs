#[test]
fn plan_entrypoints_use_shared_team_service_and_response() {
    let sources = [
        include_str!("../plan_routes/plans.rs"),
        include_str!("../../../codeterm-service-core/src/socket/server/plan.rs"),
        include_str!("../../../codeterm-service-core/src/socket/server/plan_goal.rs"),
    ];
    for source in sources {
        assert!(source.contains("create_team_plan_for_"));
        assert!(source.contains("apply_to_response"));
        assert!(!source.contains(".create_plan("));
        assert!(!source.contains(".create_plan_from_spec("));
    }
}
