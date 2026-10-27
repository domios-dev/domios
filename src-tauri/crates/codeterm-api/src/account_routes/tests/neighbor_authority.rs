use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn fixture() -> (ServiceFixture, AppState, String, String, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let other = mux.create_tab(&workspace).unwrap();
    for id in [&general, &other] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    mux.get_tab_mut(&general).unwrap().role =
        Some(codeterm_service_core::multiplexer::types::AgentRole::Manager);
    mux.get_tab_mut(&general).unwrap().role_profile =
        Some(codeterm_service_core::service::general_agent::GENERAL_AGENT_KIND.into());
    let fixture = ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().account = json!({"provider":"local"});
    let token = |id: &str| {
        state
            .svc
            .frontend_pty_env(Some(id))
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let general_token = token(&general);
    let other_token = token(&other);
    (fixture, state, general, other, general_token, other_token)
}

fn request(token: &str, command: &Value) -> axum::extract::Request {
    Request::builder()
        .method("POST")
        .uri("/api/account/neighbors/runtime")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(command.to_string()))
        .unwrap()
}

fn commands() -> [Value; 2] {
    [
        json!({"operation":"set_permission","friendship_id":"a".repeat(64),
            "permission":{"allowed":true,"scope":"opaque scope","allow_tab_requests":false}}),
        json!({"operation":"bind","friendship_id":"a".repeat(64),"gateway_tab":"target"}),
    ]
}

#[tokio::test]
async fn general_credentials_reach_neighbor_service_and_other_tabs_cannot_author() {
    let (_fixture, state, general, _, general_token, other_token) = fixture();
    let router = crate::routes::create_router(state.clone());
    for command in commands() {
        for (token, expected) in [
            (
                general_token.as_str(),
                "Sign in with a hosted account to use Neighbors",
            ),
            (
                other_token.as_str(),
                "Only your authenticated General Agent may author Neighbor permissions",
            ),
        ] {
            let response = router
                .clone()
                .oneshot(request(token, &command))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert_eq!(std::str::from_utf8(&body).unwrap(), expected);
        }
        let mut local = request("preview-test-token", &command);
        local
            .headers_mut()
            .insert("x-codeterm-tab-id", general.parse().unwrap());
        local.headers_mut().insert(
            "x-codeterm-binding-nonce",
            format!("nonce-{general}").parse().unwrap(),
        );
        let response = router.clone().oneshot(local).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            std::str::from_utf8(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap(),
            "Sign in with a hosted account to use Neighbors"
        );
    }
    let local = router
        .clone()
        .oneshot(request(
            "preview-test-token",
            &json!({"operation":"status"}),
        ))
        .await
        .unwrap();
    assert_eq!(local.status(), StatusCode::SERVICE_UNAVAILABLE);
    for path in ["/api/account/status", "/api/account/plan"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {general_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn neighbor_actor_cannot_be_replaced_by_headers_or_retired_session() {
    let (_fixture, state, general, other, general_token, other_token) = fixture();
    let router = crate::routes::create_router(state.clone());
    for command in commands() {
        for (header, claim) in [
            ("x-codeterm-tab-id", general.clone()),
            ("x-codeterm-binding-nonce", format!("nonce-{general}")),
            ("x-codeterm-session-id", format!("session-{general}")),
        ] {
            let mut forged = request(&other_token, &command);
            forged.headers_mut().insert(header, claim.parse().unwrap());
            assert_eq!(
                router.clone().oneshot(forged).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-codeterm-tab-id", other.parse().unwrap());
        headers.insert(
            "x-codeterm-binding-nonce",
            format!("nonce-{other}").parse().unwrap(),
        );
        let error = neighbor_runtime(
            State(state.clone()),
            Some(Extension(CallerIdentity::Tab(
                codeterm_service_core::service::tab_credentials::TabCaller {
                    tab_id: general.clone(),
                    binding_nonce: format!("nonce-{general}"),
                },
            ))),
            headers,
            Json(serde_json::from_value(command).unwrap()),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            std::str::from_utf8(&to_bytes(error.into_body(), 4096).await.unwrap()).unwrap(),
            "Sign in with a hosted account to use Neighbors"
        );
    }
    state
        .svc
        .mux
        .write()
        .get_tab_mut(&general)
        .unwrap()
        .agent_session_id = Some("replacement-session".into());
    assert_eq!(
        router
            .oneshot(request(&general_token, &commands()[0]))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
