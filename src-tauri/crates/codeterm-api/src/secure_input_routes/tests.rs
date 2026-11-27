use super::*;
use axum::{
    body::Body,
    http::Request,
    middleware,
    routing::{delete, post},
};
use codeterm_service_core::{
    mesh::types::{AccessLevel, PairedPeer, RelayCommand},
    multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

fn test_app(state: AppState) -> Router {
    router()
        .route("/api/secrets/items", post(crate::secrets_routes::save_item))
        .route(
            "/api/secrets/items/{name}/value",
            post(crate::secrets_routes::reveal_value),
        )
        .route(
            "/api/secrets/items/{name}",
            delete(crate::secrets_routes::delete_item),
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

fn paired_peer(id: &str, name: &str, access_level: AccessLevel) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: name.into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: String::new(),
        paired_at: "2026-01-01T00:00:00Z".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: 2,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[tokio::test]
async fn agent_can_request_and_read_status_but_cannot_submit_reveal_overwrite_or_delete() {
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

    // 6. Agent sees status=delivered, never the value, and CANNOT reveal, overwrite, or delete (P0-2).
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

    // Agent cannot overwrite a protected or reserved vault slot via save_item (P0-2).
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secrets/items",
        &agent_token,
        Some(serde_json::json!({
            "name": "deploy-pat",
            "value": "hijacked",
            "overwrite": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secrets/items",
        &agent_token,
        Some(serde_json::json!({
            "name": "domios:provider:anthropic:api_key",
            "value": "hijacked",
            "overwrite": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Agent cannot delete a protected slot (P0-2).
    let (status, _) = json_request(
        &app,
        "DELETE",
        "/api/secrets/items/deploy-pat",
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 7. Local Owner can still reveal the slot directly when needed.
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
async fn unauthorized_machine_ambiguous_name_cross_tab_provider_key_and_neighbor_are_refused() {
    let (mut mux, tab) = MultiplexerState::new();
    let ws = mux.find_workspace_for_tab(&tab).unwrap();
    let other_tab = mux.create_tab(&ws).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-token".into());

    let own_name = state.mesh.lock().status().instance_name;
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.insert(
            "peer-ro".into(),
            paired_peer("peer-ro", "readonly-box", AccessLevel::ReadOnly),
        );
        mesh.paired_peers.insert(
            "peer-shadow".into(),
            paired_peer("peer-shadow", &own_name, AccessLevel::Full),
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

    // Ambiguous machine name matching both local host and a paired peer is refused (P2-9).
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        "owner-token",
        Some(serde_json::json!({
            "machine": own_name,
            "target": "vault:ci-key",
            "kind": "key",
            "reason": "Ambiguous host name",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

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

    // Non-orchestrator agent tab cannot target another tab with ProviderKey (P1-7).
    let other_token = state
        .svc
        .frontend_pty_env(Some(&other_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/secure-input/requests",
        &other_token,
        Some(serde_json::json!({
            "target": "provider-key:anthropic",
            "tabId": tab,
            "kind": "key",
            "reason": "Cross-tab provider key attempt",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

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

#[test]
fn remote_delivery_classification_reads_bytes_written_pointer_and_locks_uncertainty() {
    // P0-1: explicit bytes_written: false -> Refused.
    let pre_write = RelayResponse {
        ok: false,
        status: 503,
        data: Some(serde_json::json!({
            "ok": false,
            "outcome": "refused",
            "error": { "bytes_written": false, "code": "peer_unreachable" }
        })),
        error: Some("peer unreachable".into()),
    };
    assert_eq!(
        classify_remote_delivery_result(Err(pre_write)).status,
        SecureInputStatus::Refused
    );

    // P0-1: absent bytes_written -> Uncertain (never replay!).
    let unknown = RelayResponse {
        ok: false,
        status: 504,
        data: Some(serde_json::json!({
            "ok": false,
            "outcome": "unknown",
            "error": { "code": "timeout" }
        })),
        error: Some("timeout".into()),
    };
    assert_eq!(
        classify_remote_delivery_result(Err(unknown)).status,
        SecureInputStatus::Uncertain
    );

    // P0-1: bytes_written: true -> Uncertain (never replay!).
    let written = RelayResponse {
        ok: false,
        status: 502,
        data: Some(serde_json::json!({
            "ok": false,
            "error": { "bytes_written": true }
        })),
        error: Some("connection reset after write".into()),
    };
    assert_eq!(
        classify_remote_delivery_result(Err(written)).status,
        SecureInputStatus::Uncertain
    );

    // P1-3: RelayCommand Debug redacts body and zeroize_sensitive_body scrubs value/code/apiKey.
    let mut cmd = RelayCommand {
        method: "POST".into(),
        path: "/api/secure-input/deliver".into(),
        body: serde_json::json!({ "value": "TOP-SECRET-RELAY-771", "code": "CODE-991" }),
        delivery_id: Some("sir_1".into()),
    };
    let debug = format!("{cmd:?}");
    assert!(!debug.contains("TOP-SECRET-RELAY-771"));
    assert!(!debug.contains("CODE-991"));
    assert!(debug.contains("<redacted>"));
    cmd.zeroize_sensitive_body();
    assert!(!cmd.body.to_string().contains("TOP-SECRET-RELAY-771"));
    assert!(!cmd.body.to_string().contains("CODE-991"));
}
