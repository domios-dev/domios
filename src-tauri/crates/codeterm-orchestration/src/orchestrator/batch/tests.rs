use super::*;

#[test]
fn orchestrator_batch_keeps_the_typed_sandbox_refusal_without_creating_a_tab() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("plain-provider"),
    ));
    let before = fixture.service.mux.read().snapshot().tabs.len();
    let result = codeterm_service_core::async_runtime::block_on(batch(
        &fixture.service,
        vec![BatchOp {
            id: None,
            method: "spawn".into(),
            params: serde_json::json!({"agent":"plain-provider","sandbox":"read-only"}),
        }],
    ));
    assert_eq!(result[0]["code"], "execution_sandbox_unsupported");
    assert_eq!(result[0]["refusal"]["provider"], "plain-provider");
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
}
