use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

fn request(method: &str, path: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn process_split_contract_router_uses_portable_state_and_acknowledges_mutation() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("portable-fixture".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    let storage = fixture.install_seeded_storage_desk();
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("portable-token".into());
    assert!(std::sync::Arc::ptr_eq(
        &state.events,
        &fixture.service.events
    ));
    let router = crate::routes::create_router_with_frontend(state.clone(), None);
    let mut events = state.events.subscribe();
    let path = format!("/api/workspaces/{workspace}/rename");
    let rejected = router
        .clone()
        .oneshot(request(
            "POST",
            &path,
            "wrong",
            serde_json::json!({"name":"unauthorized"}),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(request(
            "POST",
            &path,
            "portable-token",
            serde_json::json!({"name":"portable-renamed"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_workspace(&workspace)
            .unwrap()
            .name,
        "portable-renamed"
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if events.recv().await.unwrap().frame.event_type == "mux-state-changed" {
                break;
            }
        }
    })
    .await
    .unwrap();
    codeterm_service_core::service::session_persist::flush_session_persistence(&fixture.service)
        .unwrap();
    let persisted =
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage)
            .state()
            .unwrap();
    assert_eq!(
        persisted
            .snapshot
            .workspaces
            .iter()
            .find(|entry| entry.record.id.as_str() == workspace)
            .unwrap()
            .record
            .name,
        "portable-renamed"
    );
    let read = router
        .clone()
        .oneshot(request(
            "GET",
            "/api/workspaces",
            "portable-token",
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let native = router
        .clone()
        .oneshot(request(
            "POST",
            "/api/app/restart",
            "portable-token",
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(native.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        to_bytes(native.into_body(), 1024).await.unwrap().as_ref(),
        b"native_capability_unavailable"
    );
    let health = router
        .oneshot(request(
            "GET",
            "/api/health",
            "portable-token",
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    codeterm_service_core::service::session_persist::stop_worker(&fixture.service).unwrap();
}
