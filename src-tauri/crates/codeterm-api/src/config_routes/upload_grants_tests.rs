use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::patch,
    Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn browser_upload_grants_require_owner_credential_for_key_partial_and_full_config() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let agent = state
        .svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = Router::new()
        .route("/api/config", patch(super::patch_config_api))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone());
    let folders = json!({"work": [fixture.temp.path().join("media")]});
    let mut full = serde_json::to_value(state.config.lock().clone()).unwrap();
    full["browser_upload_folders"] = folders.clone();
    for body in [
        json!({"key":"browser_upload_folders.work", "value":folders["work"]}),
        json!({"key":"browser_upload_folders", "value":folders}),
        json!({"browser_upload_folders":folders}),
        full,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/config")
                    .header("authorization", format!("Bearer {agent}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(state.config.lock().browser_upload_folders.is_empty());
    }
    state
        .config
        .lock()
        .browser_upload_folders
        .insert("work".into(), vec!["existing".into()]);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header("authorization", format!("Bearer {agent}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"browser_upload_folders":{}}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    state.config.lock().browser_upload_folders.clear();
    let body = json!({"browser_upload_folders":{"work":["relative/folder"]}});
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header("authorization", "Bearer owner-secret")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(state.config.lock().browser_upload_folders.is_empty());
}
