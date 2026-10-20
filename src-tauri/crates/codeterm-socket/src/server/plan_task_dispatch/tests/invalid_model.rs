use super::*;

#[test]
fn plan_task_dispatch_refuses_unknown_model_before_spawn() {
    let registry = codeterm_service_core::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        codeterm_service_core::plugin::agent_claude::ClaudeProvider::new(),
    ));
    let (mut mux, _root) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let before = mux.snapshot().tabs.len();

    let refused = codeterm_service_core::service::spawn::validate_agent_model(
        &registry,
        "claude",
        Some("opuss"),
    )
    .map_err(|error| error.to_string())
    .and_then(|_| {
        let clearance = codeterm_service_core::service::spawn::provider_launch_preflight(
            &registry, "claude", false,
        )
        .map_err(|error| error.to_string())?;
        codeterm_service_core::service::spawn::create_cleared_agent_tab(
            &mut mux,
            &workspace_id,
            &clearance,
        )
        .map_err(|error| error.to_string())
    });
    let error = refused.expect_err("dispatch must refuse before reserving a Worker Tab");
    assert!(error.contains("opuss"), "{error}");
    assert!(error.contains("opus"), "{error}");
    assert_eq!(mux.snapshot().tabs.len(), before);
}
