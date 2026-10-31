use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION},
    multiplexer::{state::MultiplexerState, types::AgentRole},
    service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn web_link_token_route_authority_matrix() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    let planner = mux.create_tab(&workspace).unwrap();
    let orchestrator = mux.create_tab(&workspace).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    for id in [&general, &worker, &planner, &orchestrator, &concierge] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    mux.get_tab_mut(&general).unwrap().role = Some(AgentRole::Manager);
    mux.get_tab_mut(&general).unwrap().role_profile =
        Some(codeterm_service_core::service::general_agent::GENERAL_AGENT_KIND.into());
    mux.get_tab_mut(&worker).unwrap().role = Some(AgentRole::Worker);
    mux.get_tab_mut(&planner).unwrap().role = Some(AgentRole::Manager);
    mux.get_tab_mut(&planner).unwrap().role_profile = Some("planner".into());
    mux.get_tab_mut(&orchestrator).unwrap().role = Some(AgentRole::Manager);
    mux.get_tab_mut(&orchestrator).unwrap().role_profile = Some("orchestrator".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(serde_json::json!({"allowed":true,"scope":"opaque"})).unwrap(),
        )
        .await;
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("web-link-owner-fixture".into());
    let credential = |id: &str| {
        state
            .svc
            .frontend_pty_env(Some(id))
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let general_token = credential(&general);
    let worker_token = credential(&worker);
    let planner_token = credential(&planner);
    let orchestrator_token = credential(&orchestrator);
    let concierge_token = credential(&concierge);
    let service_id = state.svc.services_store.create(serde_json::from_value(
        serde_json::json!({"name":"web-link-fixture","kind":"daemon","health_mode":"http","push_enabled":true})
    ).unwrap()).unwrap();
    let push_token = state.svc.tokens.generate(&service_id).unwrap();
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "paired".into(),
        instance_name: "paired".into(),
        addresses: vec![],
        api_port: 7685,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        version: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    let peer = codeterm_service_core::mesh::crown::sign_request(&private_key, "paired").unwrap();
    let router = crate::routes::create_router(state.clone());
    for (name, token, expected) in [
        ("host", "web-link-owner-fixture", StatusCode::OK),
        ("general", general_token.as_str(), StatusCode::OK),
        ("worker", worker_token.as_str(), StatusCode::FORBIDDEN),
        ("planner", planner_token.as_str(), StatusCode::FORBIDDEN),
        (
            "orchestrator",
            orchestrator_token.as_str(),
            StatusCode::FORBIDDEN,
        ),
        ("concierge", concierge_token.as_str(), StatusCode::FORBIDDEN),
        ("peer", peer.as_str(), StatusCode::FORBIDDEN),
        ("service push", push_token.as_str(), StatusCode::FORBIDDEN),
        ("missing", "", StatusCode::UNAUTHORIZED),
    ] {
        let mut request = Request::builder().uri("/api/network/web-token");
        if !token.is_empty() {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{name}");
        if expected == StatusCode::OK {
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        if expected == StatusCode::OK {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(value["token"] == "web-link-owner-fixture");
        } else {
            assert!(!String::from_utf8_lossy(&body).contains("web-link-owner-fixture"));
        }
    }
    let forged = Request::builder()
        .uri("/api/network/web-token")
        .header("authorization", format!("Bearer {worker_token}"))
        .header("x-codeterm-tab-id", &general)
        .header("x-codeterm-binding-nonce", format!("nonce-{general}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(forged).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    {
        let mut mux = state.svc.mux.write();
        let tab = mux.get_tab_mut(&concierge).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile =
            Some(codeterm_service_core::service::general_agent::GENERAL_AGENT_KIND.into());
    }
    let derived = Request::builder()
        .uri("/api/network/web-token")
        .header("authorization", format!("Bearer {concierge_token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(derived).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    state
        .svc
        .mux
        .write()
        .get_tab_mut(&general)
        .unwrap()
        .agent_session_id = Some("replaced".into());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/network/web-token")
                .header("authorization", format!("Bearer {general_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
