use super::*;
use axum::{body::Body, http::Request, routing::post, Router};
use codeterm_service_core::async_runtime::block_on;
use codeterm_service_core::mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
use codeterm_service_core::service::parked_user_delivery::test_support::{
    attach_fixture_pty, await_owner_idle, note_dispatched_for_test, owner_wake_counts,
    queue_fixture, UserSubmitBoundary,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

fn request(tab: &str, token: &str, nonce: Option<&str>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{tab}/chat-send"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"text":"opaque-payload", "client_token":"one-admission",
            "startup_override_nonce":nonce})
            .to_string(),
        ))
        .unwrap()
}

fn signed_peer(state: &AppState) -> String {
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    state.config.lock().mesh.enabled = true;
    state.mesh.lock().insert_legacy_pair_for_test(PairedPeer {
        instance_id: "override-peer".into(),
        instance_name: "override-peer".into(),
        addresses: vec![],
        api_port: 0,
        public_key,
        paired_at: "fixture".into(),
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
    codeterm_service_core::mesh::crown::sign_request(&private_key, "override-peer").unwrap()
}

#[test]
fn startup_override_route_requires_authenticated_owner_and_rejected_callers_have_zero_effects() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    note_dispatched_for_test(&fixture.service, &tab);
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let tab_token = state.svc.frontend_pty_env(Some(&tab))["CODETERM_API_TOKEN"].clone();
    let peer_token = signed_peer(&state);
    let service_id = state
        .svc
        .services_store
        .create(
            serde_json::from_value(json!({
                "name":"override-push", "kind":"daemon", "health_mode":"http", "push_enabled":true,
            }))
            .unwrap(),
        )
        .unwrap();
    let push_token = state.svc.tokens.generate(&service_id).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "fixture evidence pending".into(),
            })
        }),
    );
    fixture
        .service
        .pty
        .lock()
        .invalidate_guarded_input(authority.pty_id().unwrap());
    let bare = Router::new()
        .route("/api/tabs/{id}/chat-send", post(chat_send_text))
        .with_state(state.clone());
    let router = bare.clone().layer(axum::middleware::from_fn_with_state(
        state.clone(),
        crate::auth::auth_middleware,
    ));
    let before_wakes = owner_wake_counts(&fixture.service, &tab);
    for (label, token, expected) in [
        ("tab", tab_token.as_str(), StatusCode::FORBIDDEN),
        ("peer", peer_token.as_str(), StatusCode::FORBIDDEN),
        ("push", push_token.as_str(), StatusCode::FORBIDDEN),
        ("missing", "", StatusCode::UNAUTHORIZED),
    ] {
        let response = block_on(router.clone().oneshot(request(
            &tab,
            token,
            Some(authority.binding_nonce()),
        )))
        .unwrap();
        assert_eq!(response.status(), expected, "{label}");
        assert_eq!(owner_wake_counts(&fixture.service, &tab), before_wakes);
        assert!(!fixture.service.user_authority_is_ready(&authority));
        assert!(fixture
            .service
            .pty
            .lock()
            .issue_guarded_input_permit(authority.pty_id().unwrap())
            .is_err());
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
    let missing_identity = block_on(bare.oneshot(request(
        &tab,
        "preview-test-token",
        Some(authority.binding_nonce()),
    )))
    .unwrap();
    assert_eq!(missing_identity.status(), StatusCode::FORBIDDEN);
    assert_eq!(owner_wake_counts(&fixture.service, &tab), before_wakes);
    assert!(fixture
        .service
        .reconcile_user_send_statuses(&tab)
        .unwrap()
        .is_empty());
    let response = block_on(router.oneshot(request(
        &tab,
        "preview-test-token",
        Some(authority.binding_nonce()),
    )))
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    await_owner_idle(&fixture, &tab);
    assert!(fixture.service.user_authority_is_ready(&authority));
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[test]
fn ordinary_tab_chat_send_keeps_its_existing_admission_policy_without_override() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let token = state.svc.frontend_pty_env(Some(&tab))["CODETERM_API_TOKEN"].clone();
    let router = Router::new()
        .route("/api/tabs/{id}/chat-send", post(chat_send_text))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::auth::auth_middleware,
        ));
    let response = block_on(router.oneshot(request(&tab, &token, None))).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    await_owner_idle(&fixture, &tab);
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(!fixture.service.user_authority_is_ready(&authority));
    assert_eq!(
        fixture
            .service
            .reconcile_user_send_statuses(&tab)
            .unwrap()
            .len(),
        1
    );
}
