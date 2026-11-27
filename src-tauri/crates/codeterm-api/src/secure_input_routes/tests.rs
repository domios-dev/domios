use super::*;
use axum::{body::Body, http::Request, middleware, routing::post};
use codeterm_service_core::{
    mesh::types::{AccessLevel, PairedPeer},
    multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

fn test_app(state: AppState) -> Router {
    router()
        .route(
            "/api/secrets/items/{name}/value",
            post(crate::secrets_routes::reveal_value),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state)
}

async fn json_request(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let req_body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let resp = app
        .clone()
        .oneshot(builder.body(req_body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn agent_can_request_and_read_status_but_cannot_submit_or_reveal_vault_secret() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = test_app(state.clone());

    // 1. Agent tab creates a vault secure input request.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        &agent_token,
        Some(serde_json::json!({
            "target": "vault:deploy-pat",
            "kind": "token",
            "reason": "Needed by CI release helper",
            "overwrite": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "agent request failed: {body}");
    let created: serde_json::Value = serde_json::from_str(&body).unwrap();
    let req_id = created["requestId"].as_str().unwrap().to_owned();
    assert_eq!(created["status"], "pending");

    // 2. Agent tab can read its own request status, but CANNOT list all owner requests.
    let (status, _) = json_request(
        &app,
        "GET",
        &format!("/api/secure-input/requests/{req_id}"),
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_request(
        &app,
        "GET",
        "/api/secure-input/requests",
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 3. Agent tab CANNOT submit a value for its own request.
    let secret = "SUPER-SECRET-TOKEN-88412";
    let (status, _) = json_request(
        &app,
        "POST",
        &format!("/api/secure-input/requests/{req_id}/submit"),
        &agent_token,
        Some(serde_json::json!({ "value": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 4. Local Owner submits the value once; response never contains the secret.
    let (status, submit_body) = json_request(
        &app,
        "POST",
        &format!("/api/secure-input/requests/{req_id}/submit"),
        "owner-token",
        Some(serde_json::json!({ "value": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "owner submit failed: {submit_body}");
    assert!(!submit_body.contains(secret));
    let submitted: serde_json::Value = serde_json::from_str(&submit_body).unwrap();
    assert_eq!(submitted["status"], "delivered");

    // 5. Replaying the same request is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        &format!("/api/secure-input/requests/{req_id}/submit"),
        "owner-token",
        Some(serde_json::json!({ "value": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 6. Agent sees status=delivered, never the value, and CANNOT reveal the stored vault slot.
    let (status, status_body) = json_request(
        &app,
        "GET",
        &format!("/api/secure-input/requests/{req_id}"),
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!status_body.contains(secret));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&status_body).unwrap()["status"],
        "delivered"
    );

    let (status, reveal_body) = json_request(
        &app,
        "POST",
        "/api/secrets/items/deploy-pat/value",
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!reveal_body.contains(secret));

    // 7. Owner can still reveal the slot directly when needed.
    let (status, owner_reveal) = json_request(
        &app,
        "POST",
        "/api/secrets/items/deploy-pat/value",
        "owner-token",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(owner_reveal.contains(secret));
}

#[tokio::test]
async fn unauthorized_machine_reserved_consumer_and_neighbor_or_sandbox_callers_are_refused() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());

    // Register a ReadOnly peer (must be refused for secure input).
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.insert(
            "peer-ro".into(),
            PairedPeer {
                instance_id: "peer-ro".into(),
                instance_name: "readonly-box".into(),
                addresses: Vec::new(),
                api_port: 7685,
                public_key: String::new(),
                paired_at: "2026-01-01T00:00:00Z".into(),
                last_seen: None,
                online: true,
                is_primary: false,
                access_level: AccessLevel::ReadOnly,
                domios_version: None,
                domios_build: None,
                web_ui: None,
                relay_protocol_version: 2,
                consecutive_misses: 0,
                last_active_ms: None,
            },
        );
    }

    let app = test_app(state.clone());

    // Unknown machine is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        "owner-token",
        Some(serde_json::json!({
            "machine": "nonexistent-peer",
            "target": "vault:ci-key",
            "kind": "key",
            "reason": "Need CI key",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Non-Full peer is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        "owner-token",
        Some(serde_json::json!({
            "machine": "readonly-box",
            "target": "vault:ci-key",
            "kind": "key",
            "reason": "Need CI key",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Reserved host vault slot is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        "owner-token",
        Some(serde_json::json!({
            "target": "vault:domios.instance:api_token",
            "kind": "token",
            "reason": "Attempt host overwrite",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Arbitrary / non-closed consumer is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        "owner-token",
        Some(serde_json::json!({
            "target": "shell:tab-1",
            "kind": "password",
            "reason": "Attempt arbitrary target",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Neighbor tab is refused even for a local vault slot.
    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    {
        let mut live_mux = state.svc.mux.write();
        live_mux.get_tab_mut(&tab).unwrap().neighbor_authority = Some(
            codeterm_service_core::multiplexer::types::NeighborAuthority {
                uid: "owner".into(),
                friendship_id: "a".repeat(64),
                root_tab_id: None,
            },
        );
    }
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        &agent_token,
        Some(serde_json::json!({
            "target": "vault:neighbor-attempt",
            "kind": "token",
            "reason": "Neighbor trying to prompt owner",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn deliver_endpoint_enforces_owner_authority_and_single_use_dedupe() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());
    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let own_id = state.mesh.lock().status().instance_id;
    let app = test_app(state.clone());

    let payload = serde_json::json!({
        "requestId": "sir_deliver_once",
        "targetInstallationId": own_id,
        "consumer": { "type": "vault_slot", "name": "remote-delivered-pat", "overwrite": true },
        "kind": "token",
        "expiresAtMs": secure_input::now_ms() + 60_000,
        "value": "SECRET-DELIVERED-ONCE-441",
    });

    // Agent tab cannot call /api/secure-input/deliver directly.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/deliver",
        &agent_token,
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Owner call succeeds once.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/secure-input/deliver",
        "owner-token",
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "deliver failed: {body}");
    assert!(!body.contains("SECRET-DELIVERED-ONCE-441"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["status"],
        "delivered"
    );

    // Replay of same requestId is refused.
    let (status, replay_body) = json_request(
        &app,
        "POST",
        "/api/secure-input/deliver",
        "owner-token",
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&replay_body).unwrap()["status"],
        "refused"
    );
}
