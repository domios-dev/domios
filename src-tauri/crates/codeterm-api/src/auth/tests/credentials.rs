// WHAT: authenticated caller identity across REST routes and launch retirement.
// WHY: request headers and body fields previously impersonated other tabs.
// ORACLE: own credentials work; foreign claims and retired credentials refuse.
use super::super::*;
use axum::{body::Body, routing::get, Extension, Json, Router};
use codeterm_service_core::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use serde_json::{json, Value};
use tower::ServiceExt;

fn fixture() -> (ServiceFixture, AppState, String, String, String) {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let second = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let token = state.svc.frontend_pty_env(Some(&first)).remove("CODETERM_API_TOKEN").unwrap();
    (fixture, state, first, second, token)
}

fn request(method: &str, path: &str, token: &str, body: Value) -> Request {
    Request::builder().method(method).uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
}

async fn identity(Extension(caller): Extension<CallerIdentity>, headers: axum::http::HeaderMap) -> Json<Value> {
    match caller {
        CallerIdentity::Tab(caller) => Json(json!({"tab":caller.tab_id,"nonce":caller.binding_nonce,
            "header":headers.get("x-codeterm-tab-id").unwrap().to_str().unwrap()})),
        CallerIdentity::Local => Json(json!({"owner":true})),
        _ => panic!("unexpected caller"),
    }
}

#[tokio::test]
async fn headers_derive_from_credentials_and_cross_tab_claims_refuse() {
    let (_fixture, state, tab, other, token) = fixture();
    let router = Router::new().route("/api/probe", get(identity))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));
    let response = router.clone().oneshot(request("GET", "/api/probe", &token, Value::Null)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["tab"], tab);
    assert_eq!(body["header"], tab);
    let mut forged = request("GET", "/api/probe", &token, Value::Null);
    forged.headers_mut().insert("x-codeterm-tab-id", other.parse().unwrap());
    assert_eq!(router.clone().oneshot(forged).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut stale = request("GET", "/api/probe", &token, Value::Null);
    stale.headers_mut().insert("x-codeterm-binding-nonce", "old-nonce".parse().unwrap());
    assert_eq!(router.clone().oneshot(stale).await.unwrap().status(), StatusCode::FORBIDDEN);
    let owner = router.oneshot(request("GET", "/api/probe", "owner-secret", Value::Null)).await.unwrap();
    assert_eq!(owner.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(owner.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body, json!({"owner":true}));
}

#[tokio::test]
async fn rest_refuses_revoked_launch_and_closed_tab_tokens() {
    let (_fixture, state, tab, _, token) = fixture();
    let router = Router::new().route("/api/probe", get(identity))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));
    state.svc.rotate_session_binding_nonce(&tab);
    assert_eq!(router.clone().oneshot(request("GET", "/api/probe", &token, Value::Null)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    let current = state.svc.frontend_pty_env(Some(&tab)).remove("CODETERM_API_TOKEN").unwrap();
    assert_eq!(router.clone().oneshot(request("GET", "/api/probe", &current, Value::Null)).await.unwrap().status(), StatusCode::OK);
    state.svc.close_tab(&tab).unwrap();
    assert_eq!(router.oneshot(request("GET", "/api/probe", &current, Value::Null)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn portable_import_report_batch_and_hooks_cannot_borrow_another_tabs_authority() {
    let (_fixture, state, tab, other, token) = fixture();
    let router = crate::routes::create_router_with_frontend(state, None);
    for (path, body) in [
        ("/api/automation/blueprints/import".to_string(), json!({"caller_tab_id":other,"recipe":{},"assignments":[]})),
        (format!("/api/tabs/{other}/report"), json!({"message":"opaque report","caller_tab_id":other})),
        ("/api/batch".to_string(), json!({"commands":[]})),
        ("/api/plans/missing/batch".to_string(), json!({"caller_tab_id":other,"ops":[]})),
        ("/api/plans/missing/decision".to_string(), json!({"asked_by_tab_id":other,"question":"opaque"})),
        ("/api/plans/missing/decision/gate/answer".to_string(), json!({"answered_by_tab_id":other,"answer":"opaque"})),
        ("/api/pty/spawn".to_string(), json!({"tab_id":other,"cols":80,"rows":24})),
        (format!("/api/tabs/{other}/hook/session_start"), json!({})),
    ] {
        assert_eq!(router.clone().oneshot(request("POST", &path, &token, body)).await.unwrap().status(), StatusCode::FORBIDDEN, "{path}");
    }
    let own = router.clone().oneshot(request("POST", "/api/automation/blueprints/import", &token,
        json!({"caller_tab_id":tab,"recipe":{},"assignments":[]}))).await.unwrap();
    assert_eq!(own.status(), StatusCode::BAD_REQUEST);
    let owner = router.oneshot(request("POST", "/api/batch", "owner-secret", json!({"commands":[]}))).await.unwrap();
    assert_eq!(owner.status(), StatusCode::OK);
}

#[tokio::test]
async fn owner_and_tab_config_reads_never_disclose_the_host_credential() {
    let (_fixture, state, _, _, token) = fixture();
    let router = crate::routes::create_router_with_frontend(state, None);
    for credential in ["owner-secret", token.as_str()] {
        let response = router.clone().oneshot(request("GET", "/api/config", credential, Value::Null)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["api"]["token"].is_null());
        assert!(!String::from_utf8_lossy(&bytes).contains("owner-secret"));
    }
}

#[tokio::test]
async fn http_rpc_refuses_a_cross_tab_actor_before_dispatch() {
    let (_fixture, state, _, other, token) = fixture();
    let router = crate::routes::create_router_with_frontend(state, None);
    let mut forged = request("POST", "/api/cli/rpc", &token,
        json!({"method":"agent.report","params":{"tab_id":other,"message":"opaque"},"protocol_version":0}));
    forged.extensions_mut().insert(ConnectInfo("127.0.0.1:41000".parse::<SocketAddr>().unwrap()));
    assert_eq!(router.oneshot(forged).await.unwrap().status(), StatusCode::FORBIDDEN);
}
