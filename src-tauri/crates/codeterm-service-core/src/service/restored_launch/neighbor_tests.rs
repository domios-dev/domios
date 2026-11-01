use super::*;

#[test]
fn concierge_capture_persists_effective_initial_instructions_through_restore_serialization() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&concierge).unwrap().neighbor_authority =
        Some(crate::multiplexer::types::NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let registry = &fixture.service.agent_registry;
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("capture-concierge")
            .with_session_config_delivery(),
    ));
    let effective = "opaque-effective-initial-context-319 with opaque-grant-7f9";
    let launch = capture_tab_launch_parameters(
        &fixture.service.mux,
        &concierge,
        registry,
        RestorableLaunchCapture {
            provider_id: "capture-concierge",
            requested_model: None,
            model: None,
            reasoning_effort: None,
            args: None,
            agent_name: None,
            task: Some(effective),
            role: None,
            role_profile: None,
            permission_bypass_requested: false,
        },
    );
    assert_eq!(launch.identity_instructions.as_deref(), Some(effective));
    let restored: crate::multiplexer::types::RestorableLaunchParameters =
        serde_json::from_value(serde_json::to_value(&launch).unwrap()).unwrap();
    assert_eq!(restored.identity_instructions.as_deref(), Some(effective));
    let (_, restored_task) =
        restored_launch_task(registry, "capture-concierge", None, None, &restored);
    assert!(restored_task.unwrap().contains(effective));
}
