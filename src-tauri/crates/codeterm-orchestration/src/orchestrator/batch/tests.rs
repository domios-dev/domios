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

#[tokio::test(flavor = "current_thread")]
async fn in_review_batch_report_returns_its_refusal_without_panicking_the_async_worker() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let results = batch(
        &fixture.service,
        vec![
            BatchOp {
                id: None,
                method: "report".into(),
                params: serde_json::json!({
                    "tab_id": "missing-worker",
                    "message": "opaque-report",
                    "status": "in_review",
                }),
            },
            BatchOp {
                id: None,
                method: "notify".into(),
                params: serde_json::json!({"title": "opaque-follow-up"}),
            },
        ],
    )
    .await;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["method"], "report");
    let error = results[0]["error"].as_str().unwrap();
    assert!(!error.contains("blocking service task failed"), "{error}");
    assert_eq!(results[1]["ok"], true);
    assert!(results[1]["id"].as_str().is_some_and(|id| !id.is_empty()));
}
