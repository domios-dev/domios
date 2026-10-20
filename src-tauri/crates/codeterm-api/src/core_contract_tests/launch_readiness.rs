#[test]
fn launch_readiness_mesh_relay_reaches_the_same_spawn_contract() {
    let relay = include_str!("../mesh_routes/relay.rs");
    let routes = include_str!("../agent_routes.rs");

    assert!(relay.contains("/api/agents/spawn"));
    assert!(routes.contains(".spawn_agent(params, ready_tx)"));
    for source in [relay, routes] {
        assert!(!source.contains("launch_readiness"));
        assert!(!source.contains("BootStage"));
    }
}
