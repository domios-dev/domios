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

fn commands() -> [Value; 7] {
    [
        json!({"operation":"set_permission","friendship_id":"a".repeat(64),
            "permission":{"allowed":true,"scope":"opaque scope","allow_tab_requests":false}}),
        json!({"operation":"bind","friendship_id":"a".repeat(64),"gateway_tab":"target"}),
        json!({"operation":"permission","friendship_id":"a".repeat(64)}),
        json!({"operation":"change_agent","friendship_id":"a".repeat(64),"provider":"fixture","model":null}),
        json!({"operation":"set_daily_limit","friendship_id":"a".repeat(64),"daily_limit":null}),
        json!({"operation":"notification_preference","friendship_id":"a".repeat(64)}),
        json!({"operation":"set_notification_preference","friendship_id":"a".repeat(64),"notify":"in_app_and_bridges"}),
    ]
}

#[tokio::test]
async fn only_general_may_list_and_set_hourly_limit_while_other_registry_mutations_stay_owner_only()
{
    let (_fixture, state, _, _, general, other) = fixture();
    let router = crate::routes::create_router(state);
    for (token, operation, status) in [
        (general.as_str(), "list", StatusCode::SERVICE_UNAVAILABLE),
        (other.as_str(), "list", StatusCode::FORBIDDEN),
        (
            general.as_str(),
            "set_limit",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (other.as_str(), "set_limit", StatusCode::FORBIDDEN),
        (general.as_str(), "request", StatusCode::FORBIDDEN),
        (general.as_str(), "respond", StatusCode::FORBIDDEN),
        (general.as_str(), "remove", StatusCode::FORBIDDEN),
    ] {
        let command = match operation {
            "list" => json!({"operation":"list"}),
            "set_limit" => {
                json!({"operation":"set_limit","friendship_id":"a".repeat(64),"per_hour":2})
            }
            "request" => json!({"operation":"request","email":"peer@example.test"}),
            "respond" => json!({"operation":"respond","request_id":"opaque","accept":true}),
            _ => json!({"operation":"remove","friendship_id":"a".repeat(64)}),
        };
        let mut request = request(token, &command);
        *request.uri_mut() = "/api/account/neighbors".parse().unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), status);
        if matches!(operation, "list" | "set_limit") && token == general {
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert_eq!(
                std::str::from_utf8(&body).unwrap(),
                "Sign in to manage Neighbors"
            );
        }
    }
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
            (other_token.as_str(), "neighbor_forbidden"),
        ] {
            let response = router
                .clone()
                .oneshot(request(token, &command))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if token == other_token {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            );
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            if token == other_token {
                assert_eq!(
                    serde_json::from_slice::<Value>(&body).unwrap()["reason"],
                    expected
                );
            } else {
                assert_eq!(std::str::from_utf8(&body).unwrap(), expected);
            }
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
        let owner_without_tab = router
            .clone()
            .oneshot(request("preview-test-token", &command))
            .await
            .unwrap();
        assert_eq!(owner_without_tab.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            std::str::from_utf8(&to_bytes(owner_without_tab.into_body(), 4096).await.unwrap())
                .unwrap(),
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
async fn unrelated_tabs_are_refused_all_runtime_surfaces_and_lifecycle_is_owner_only() {
    let (_fixture, state, _, _, general, other) = fixture();
    let router = crate::routes::create_router(state);
    for command in [
        json!({"operation":"status"}),
        json!({"operation":"history","friendship_id":"a".repeat(64)}),
        json!({"operation":"send","friendship_id":"a".repeat(64),"kind":"question","body":"opaque"}),
        json!({"operation":"delivery_status","message_id":"b".repeat(32)}),
        json!({"operation":"retry","friendship_id":"a".repeat(64)}),
        json!({"operation":"own_keys"}),
        json!({"operation":"enroll","takeover_confirmed":true,"expected_receiver_revision":0}),
        json!({"operation":"rotate"}),
        json!({"operation":"revoke"}),
    ] {
        let response = router
            .clone()
            .oneshot(request(&other, &command))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["reason"], "neighbor_forbidden");
        if matches!(
            command["operation"].as_str(),
            Some("enroll" | "rotate" | "revoke")
        ) {
            assert_eq!(
                router
                    .clone()
                    .oneshot(request(&general, &command))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
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
