use super::{api_listener_value, api_query_includes_listener, redact_rest_config};
use axum::{
    body::{to_bytes, Body},
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
async fn config_patch_refuses_wallpaper_writes_and_points_to_dedicated_route() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let installed = json!({"background": {"pattern": "dots"}});
    {
        let mut config = state.config.lock();
        config.api.token = Some("owner-secret".into());
        config.chat_wallpaper = Some(serde_json::from_value(installed.clone()).unwrap());
    }
    let app = Router::new()
        .route("/api/config", patch(super::patch_config_api))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone());
    let replacement = json!({"background": {"pattern": "none"}});
    for body in [
        json!({"key": "chat_wallpaper", "value": replacement}),
        json!({"key": "chat_wallpaper.background.pattern", "value": "none"}),
        json!({"key": "chat_wallpaper", "value": null}),
        json!({"chat_wallpaper": replacement}),
        json!({"chat_wallpaper": null}),
    ] {
        let response = app
            .clone()
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply["error"]["code"], "wallpaper_route_required");
        assert_eq!(reply["error"]["method"], "PUT");
        assert_eq!(reply["error"]["path"], "/api/chat-wallpaper");
        assert_eq!(
            serde_json::to_value(&state.config.lock().chat_wallpaper).unwrap(),
            installed
        );
    }
}

#[test]
fn rest_config_never_serializes_api_token() {
    let config = serde_json::json!({
        "api": { "host": "0.0.0.0", "token": "permanent-secret" },
        "mesh": { "enabled": true }
    });
    let redacted = redact_rest_config(config);
    assert_eq!(redacted["api"]["token"], serde_json::Value::Null);
    assert_eq!(redacted["api"]["host"], "0.0.0.0");
    assert_eq!(redacted["mesh"]["enabled"], true);
}

#[test]
fn api_listener_readback_preserves_active_policy_until_restart() {
    let mut cfg = codeterm_service_core::config::schema::AppConfig::default();
    cfg.api.enabled = true;
    cfg.api.host = "localhost".into();
    cfg.api.port = 7686;
    cfg.api.cors = true;
    cfg.api.cors_allow_origins = vec!["https://configured.example".into()];
    let active = codeterm_service_core::startup::RestApiRuntimePolicy {
        enabled: true,
        cors: false,
        cors_allow_origins: Vec::new(),
    };

    let disclosure = api_listener_value(Some("127.0.0.1:7686".parse().unwrap()), active, &cfg);
    assert_eq!(disclosure["active"]["cors"], false);
    assert_eq!(
        disclosure["active"]["cors_allow_origins"],
        serde_json::json!([])
    );
    assert_eq!(disclosure["configured"]["cors"], true);
    assert_eq!(
        disclosure["configured"]["cors_allow_origins"],
        serde_json::json!(["https://configured.example"])
    );
    assert_eq!(disclosure["restart_required"], true);
}

#[test]
fn exact_api_runtime_queries_include_listener_truth() {
    for key in ["api", "api.enabled", "api.cors", "api.cors_allow_origins"] {
        assert!(
            api_query_includes_listener(key),
            "missing listener truth for {key}"
        );
    }
    assert!(!api_query_includes_listener("theme.name"));
}
