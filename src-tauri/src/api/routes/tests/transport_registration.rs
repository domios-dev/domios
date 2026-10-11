use crate::api::routes::create_router;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[path = "../../transport_tabs/fixture.rs"]
mod fixture;

#[test]
fn transport_domain_registration_contract_routers_merge_without_collisions() {
    let _: axum::Router<crate::api::routes::AppState> =
        super::super::domain_transport_routes(axum::Router::new());
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn transport_domain_registration_contract_auth_methods_and_persisted_cwd() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let app = create_router(fixture::app_state(&fixture));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["apiVersion"],
        2
    );
    let routes = [
        ("POST", format!("/api/tabs/{tab}/cwd")),
        ("GET", format!("/api/tabs/{tab}/history")),
        ("GET", "/api/agents/providers/unknown/model-usage".into()),
        ("GET", "/api/system/default-cwd".into()),
        ("GET", "/api/system/shell-diagnostics".into()),
        ("POST", "/api/system/install-pwsh".into()),
        ("GET", "/api/themes/location".into()),
        ("POST", "/api/attachments/path-kinds".into()),
        ("POST", "/api/diagnostics/webgl".into()),
    ];
    for (method, path) in routes {
        for token in [None, Some("Bearer wrong")] {
            let mut request = Request::builder().method(method).uri(&path);
            if let Some(token) = token {
                request = request.header("authorization", token);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header("authorization", "Bearer preview-test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{tab}/cwd"))
        .header("authorization", "Bearer preview-test-token")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"cwd":"/transport-contract"}"#))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .cwd
            .as_deref(),
        Some("/transport-contract")
    );
    let request = Request::builder()
        .uri("/api/agents/providers/unknown/model-usage")
        .header("authorization", "Bearer preview-test-token")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!([])
    );
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn transport_domain_registration_contract_team_nonce_and_persisted_rename() {
    let (mut mux, caller) = MultiplexerState::new();
    let team = mux
        .resolve_or_mint_team_for_parent(&caller)
        .unwrap()
        .team_id;
    mux.get_tab_mut(&caller).unwrap().session_binding_nonce = Some("current-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let app = create_router(fixture::app_state(&fixture));
    for (nonce, header_tab, header_nonce) in [
        (None, None, None),
        (Some("stale"), None, None),
        (Some("current-nonce"), Some("foreign-tab"), None),
        (Some("current-nonce"), Some(caller.as_str()), Some("stale")),
    ] {
        let body = serde_json::json!({
            "name": "Refused", "caller_tab_id": caller, "session_binding_nonce": nonce,
        });
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/teams/{team}/rename"))
            .header("authorization", "Bearer preview-test-token")
            .header("content-type", "application/json");
        if let Some(tab) = header_tab {
            request = request.header("X-CodeTerm-Tab-Id", tab);
        }
        if let Some(nonce) = header_nonce {
            request = request.header("X-CodeTerm-Binding-Nonce", nonce);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            fixture.service.mux.read().get_team(&team).unwrap().name,
            None
        );
    }
    let body = serde_json::json!({
        "name": "Accepted", "caller_tab_id": caller, "session_binding_nonce": "current-nonce",
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/teams/{team}/rename"))
                .header("authorization", "Bearer preview-test-token")
                .header("content-type", "application/json")
                .header("X-CodeTerm-Tab-Id", &caller)
                .header("X-CodeTerm-Binding-Nonce", "current-nonce")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_team(&team)
            .unwrap()
            .name
            .as_deref(),
        Some("Accepted")
    );
    for path in [
        format!("/api/teams/{team}/rename"),
        format!("/api/teams/{team}/close"),
        format!("/api/teams/{team}/members"),
        "/api/orchestration/new".into(),
        format!("/api/tabs/{caller}/make-orchestrator"),
        format!("/api/tabs/{caller}/make-planner"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        for nonce in [None, Some("stale")] {
            let body =
                serde_json::json!({ "caller_tab_id": caller, "session_binding_nonce": nonce });
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&path)
                        .header("authorization", "Bearer preview-test-token")
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(
                fixture
                    .service
                    .mux
                    .read()
                    .get_team(&team)
                    .unwrap()
                    .name
                    .as_deref(),
                Some("Accepted")
            );
            assert!(fixture.service.mux.read().get_tab(&caller).is_some());
        }
    }
}
