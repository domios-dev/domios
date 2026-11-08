use super::*;
use axum::{body::Body, http::Request, middleware};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn runtime_login_read_requires_authentication_and_never_creates_a_runtime() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("test-token".into());
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_state
        .is_none());
}

#[test]
fn runtime_login_response_preserves_opaque_values_without_http_caching() {
    let value = serde_json::json!({"login":{"status":"pending","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000}});
    let (headers, Json(body)) = login_response(value);
    assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-store");
    assert_eq!(body["login"]["url"], "https://example.invalid/opaque-path");
    assert_eq!(body["login"]["userCode"], "opaque-token");
    assert_eq!(body["login"]["expiresAtMs"], 5000);
}

#[tokio::test]
async fn agents_cannot_change_a_mode_or_host_approvals_and_the_owner_can() {
    let (mut mux, tab) = MultiplexerState::new();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-thread".into());
        target.session_binding_nonce = Some("own-nonce".into());
        target.reported_session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let agent = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);
    let status = |token: String, path: String, body: serde_json::Value| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    };
    let controls = format!("/api/tabs/{tab}/agent/runtime/controls");
    let approvals = format!("/api/tabs/{tab}/agent/runtime/host-approvals");
    let selection = |mode: bool| {
        serde_json::json!({"threadId":"own-thread","bindingNonce":"own-nonce","epoch":1,
            "id":"opaque-control","value":"opaque-approve-all","mode":mode})
    };
    let allow = serde_json::json!({"allowAll":true});

    assert_eq!(
        status(agent.clone(), controls.clone(), selection(true)).await,
        StatusCode::FORBIDDEN,
        "an agent must not select a mode for itself"
    );
    assert_eq!(
        status(agent.clone(), approvals.clone(), allow.clone()).await,
        StatusCode::FORBIDDEN,
        "an agent must not lift its own Ask"
    );
    // Past the authority check, both reach the service and find no runtime or capability.
    assert_eq!(
        status(agent, controls.clone(), selection(false)).await,
        StatusCode::CONFLICT,
        "other controls stay open to agents"
    );
    assert_eq!(
        status("owner-token".into(), controls, selection(true)).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        status("owner-token".into(), approvals, allow).await,
        StatusCode::CONFLICT
    );
}
