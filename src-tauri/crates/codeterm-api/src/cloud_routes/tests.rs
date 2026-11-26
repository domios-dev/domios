use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    mesh::{
        crown::{
            generate_peer_keypair, sign_request_with_agent, sign_request_with_origin,
            AuthenticatedAgent, AuthenticatedOrigin,
        },
        types::{AccessLevel, PairedPeer, Permission, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
    service::{
        cloud_machines::{
            CloudError, CloudPromoRedemption, CloudProvisioningPort, CloudSleepMode,
            CloudUsageSummary, ProvisionedLease, ProvisionedMachine, ProvisioningCreateRequest,
        },
        test_fixture::ServiceFixture,
    },
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::CloudProvisioningOverride;
use crate::routes;

struct FakePromoProvisioning {
    seen_codes: Mutex<Vec<String>>,
    outcomes: Mutex<VecDeque<Result<CloudPromoRedemption, CloudError>>>,
}

impl FakePromoProvisioning {
    fn new(outcomes: Vec<Result<CloudPromoRedemption, CloudError>>) -> Arc<Self> {
        Arc::new(Self {
            seen_codes: Mutex::new(Vec::new()),
            outcomes: Mutex::new(outcomes.into()),
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for FakePromoProvisioning {
    async fn create(
        &self,
        _: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn wake(&self, _: &str, _: &str) -> Result<ProvisionedMachine, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn sleep(&self, _: &str, _: &str) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::Stop)
    }

    async fn delete(&self, _: &str, _: &str) -> Result<(), CloudError> {
        Ok(())
    }

    async fn renew_lease(&self, _: &str, _: &str, _: &str) -> Result<ProvisionedLease, CloudError> {
        Err(CloudError::Internal("unused".into()))
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }

    async fn redeem_promo(&self, code: &str) -> Result<CloudPromoRedemption, CloudError> {
        self.seen_codes.lock().push(code.to_owned());
        self.outcomes
            .lock()
            .pop_front()
            .unwrap_or(Err(CloudError::PromoInvalidCode))
    }
}

async fn post_redeem(app: axum::Router, token: &str, payload: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mesh/cloud/redeem")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
    )
}

#[tokio::test]
async fn cloud_redeem_route_maps_success_idempotency_typed_refusals_and_local_authority() {
    crate::register_api_extensions();
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let (peer_priv, peer_pub) = generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "remote-peer".into(),
        instance_name: "RemotePeer".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: peer_pub,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });

    let provisioning = FakePromoProvisioning::new(vec![
        Ok(CloudPromoRedemption {
            schema_version: 1,
            status: "redeemed".into(),
            already_redeemed: false,
            hours: 25.0,
            expires_at: Some(1_800_000_000),
            remaining: 35.0,
            purchased: 25.0,
        }),
        Ok(CloudPromoRedemption {
            schema_version: 1,
            status: "redeemed".into(),
            already_redeemed: true,
            hours: 25.0,
            expires_at: None,
            remaining: 35.0,
            purchased: 25.0,
        }),
        Err(CloudError::PromoInvalidCode),
        Err(CloudError::PromoExpired),
        Err(CloudError::PromoDisabled),
        Err(CloudError::PromoExhausted),
        Err(CloudError::PromoSpendCapExceeded),
        Err(CloudError::RateLimited {
            retry_after_seconds: 120,
        }),
    ]);
    assert!(state
        .svc
        .set_ext(CloudProvisioningOverride(provisioning.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(state, None);

    // Remote peer caller is refused (local-only mutation).
    let peer_token =
        sign_request_with_origin(&peer_priv, "remote-peer", AuthenticatedOrigin::Owner).unwrap();
    let (peer_status, _) =
        post_redeem(app.clone(), &peer_token, json!({ "code": "BETA-25" })).await;
    assert_eq!(peer_status, StatusCode::FORBIDDEN);
    assert!(provisioning.seen_codes.lock().is_empty());

    // Blank code is rejected locally as invalid_code before calling backend.
    let (blank_status, blank_body) =
        post_redeem(app.clone(), "owner-secret", json!({ "code": "   " })).await;
    assert_eq!(blank_status, StatusCode::BAD_REQUEST);
    assert_eq!(blank_body["error"]["code"], "invalid_code");
    assert!(provisioning.seen_codes.lock().is_empty());

    // Fresh redemption returns 200 with exact wire fields.
    let (status, fresh) =
        post_redeem(app.clone(), "owner-secret", json!({ "code": " BETA-25 " })).await;
    assert_eq!(status, StatusCode::OK, "{fresh}");
    assert_eq!(fresh["schema_version"], 1);
    assert_eq!(fresh["status"], "redeemed");
    assert_eq!(fresh["already_redeemed"], false);
    assert_eq!(fresh["hours"], 25.0);
    assert_eq!(fresh["expires_at"], 1_800_000_000u64);
    assert_eq!(fresh["remaining"], 35.0);
    assert_eq!(fresh["purchased"], 25.0);
    assert_eq!(provisioning.seen_codes.lock().as_slice(), &["BETA-25"]);

    // Idempotent repeat redemption preserves already_redeemed=true and expires_at=null.
    let (status, repeat) =
        post_redeem(app.clone(), "owner-secret", json!({ "code": "BETA-25" })).await;
    assert_eq!(status, StatusCode::OK, "{repeat}");
    assert_eq!(repeat["already_redeemed"], true);
    assert!(repeat["expires_at"].is_null());

    // All 6 typed promo failure reasons map cleanly for Switch/UI and CLI.
    for (expected_status, expected_code) in [
        (StatusCode::BAD_REQUEST, "invalid_code"),
        (StatusCode::BAD_REQUEST, "expired"),
        (StatusCode::BAD_REQUEST, "disabled"),
        (StatusCode::CONFLICT, "exhausted"),
        (StatusCode::CONFLICT, "spend_cap_exceeded"),
    ] {
        let (st, body) =
            post_redeem(app.clone(), "owner-secret", json!({ "code": "TEST-CODE" })).await;
        assert_eq!(st, expected_status, "{body}");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], expected_code);
        assert!(body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()));
    }

    let (rl_status, rl_body) =
        post_redeem(app, "owner-secret", json!({ "code": "TEST-CODE" })).await;
    assert_eq!(rl_status, StatusCode::TOO_MANY_REQUESTS, "{rl_body}");
    assert_eq!(rl_body["error"]["code"], "rate_limited");
    assert_eq!(rl_body["error"]["retry_after_seconds"], 120);
}

fn paired_peer_with_access(
    id: &str,
    name: &str,
    public_key: String,
    access_level: AccessLevel,
) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: name.into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: &str,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let body = if method == "POST" {
        builder = builder.header("content-type", "application/json");
        Body::from("{}")
    } else {
        Body::empty()
    };
    let response = app.oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
    )
}

#[tokio::test]
async fn cloud_peer_control_routes_enforce_owner_and_full_peer_authority_only() {
    crate::register_api_extensions();
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let agent_token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();

    let (full_priv, full_pub) = generate_peer_keypair();
    let (admin_priv, admin_pub) = generate_peer_keypair();
    let (ro_priv, ro_pub) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-full",
            "FullPeer",
            full_pub,
            AccessLevel::Full,
        ));
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-custom-admin",
            "CustomAdminPeer",
            admin_pub,
            AccessLevel::Custom(vec![Permission::Admin]),
        ));
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-readonly",
            "ReadOnlyPeer",
            ro_pub,
            AccessLevel::ReadOnly,
        ));
    }

    let drain_gate = state.svc.cloud_machines().local_drain_gate().clone();
    let app = routes::create_router_with_frontend(state, None);

    let full_agent = AuthenticatedAgent {
        instance_id: "peer-full".into(),
        tab_id: "remote-agent-tab".into(),
        can_spawn: false,
    };

    // Unauthorized callers must be refused 403 on idle, drain, and reopen without mutating gate.
    for (method, path) in [
        ("GET", "/api/mesh/cloud/peer/idle"),
        ("POST", "/api/mesh/cloud/peer/drain"),
        ("POST", "/api/mesh/cloud/peer/reopen"),
    ] {
        let (tab_status, _) = request_json(app.clone(), method, path, &agent_token).await;
        assert_eq!(
            tab_status,
            StatusCode::FORBIDDEN,
            "local agent tab must be refused on {method} {path}"
        );

        let full_agent_token = sign_request_with_agent(
            &full_priv,
            "peer-full",
            AuthenticatedOrigin::Agent,
            Some(&full_agent),
        )
        .unwrap();
        let (agent_status, _) = request_json(app.clone(), method, path, &full_agent_token).await;
        assert_eq!(
            agent_status,
            StatusCode::FORBIDDEN,
            "Full peer with Agent origin must be refused on {method} {path}"
        );

        let custom_admin_token =
            sign_request_with_origin(&admin_priv, "peer-custom-admin", AuthenticatedOrigin::Owner)
                .unwrap();
        let (admin_status, _) = request_json(app.clone(), method, path, &custom_admin_token).await;
        assert_eq!(
            admin_status,
            StatusCode::FORBIDDEN,
            "Custom(Admin) peer must be refused on {method} {path}"
        );

        let ro_token =
            sign_request_with_origin(&ro_priv, "peer-readonly", AuthenticatedOrigin::Owner)
                .unwrap();
        let (ro_status, _) = request_json(app.clone(), method, path, &ro_token).await;
        assert_eq!(
            ro_status,
            StatusCode::FORBIDDEN,
            "ReadOnly peer must be refused on {method} {path}"
        );
    }
    assert!(
        !drain_gate.is_draining(),
        "refused drain attempts must not flip the drain gate"
    );

    // Local Owner can inspect idle, enter drain, and reopen.
    let (status, idle_body) = request_json(
        app.clone(),
        "GET",
        "/api/mesh/cloud/peer/idle",
        "owner-secret",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{idle_body}");
    assert_eq!(idle_body["draining"], false);

    let (status, drain_body) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/drain",
        "owner-secret",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{drain_body}");
    assert!(drain_gate.is_draining());

    // While draining, local agent tab and remote agent on Full peer cannot reopen mid-sleep.
    let (tab_reopen, _) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/reopen",
        &agent_token,
    )
    .await;
    assert_eq!(tab_reopen, StatusCode::FORBIDDEN);
    let full_agent_reopen_tok = sign_request_with_agent(
        &full_priv,
        "peer-full",
        AuthenticatedOrigin::Agent,
        Some(&full_agent),
    )
    .unwrap();
    let (remote_agent_reopen, _) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/reopen",
        &full_agent_reopen_tok,
    )
    .await;
    assert_eq!(remote_agent_reopen, StatusCode::FORBIDDEN);
    assert!(
        drain_gate.is_draining(),
        "refused reopen attempts must leave the drain gate closed mid-sleep"
    );

    // Full peer with Mesh origin can reopen; Full peer with Owner origin can drain and reopen.
    let full_mesh_reopen_tok =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Mesh).unwrap();
    let (mesh_reopen, _) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/reopen",
        &full_mesh_reopen_tok,
    )
    .await;
    assert_eq!(mesh_reopen, StatusCode::OK);
    assert!(!drain_gate.is_draining());

    let full_owner_drain_tok =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    let (owner_drain, _) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/drain",
        &full_owner_drain_tok,
    )
    .await;
    assert_eq!(owner_drain, StatusCode::OK);
    assert!(drain_gate.is_draining());

    let full_owner_reopen_tok =
        sign_request_with_origin(&full_priv, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    let (owner_reopen, _) = request_json(
        app,
        "POST",
        "/api/mesh/cloud/peer/reopen",
        &full_owner_reopen_tok,
    )
    .await;
    assert_eq!(owner_reopen, StatusCode::OK);
    assert!(!drain_gate.is_draining());
}

#[tokio::test]
async fn cloud_peer_idle_counts_only_local_owner_and_full_peer_viewers() {
    crate::register_api_extensions();
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let (full_priv, full_pub) = generate_peer_keypair();
    let (ro_priv, ro_pub) = generate_peer_keypair();
    let (iso_priv, iso_pub) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-full-viewer",
            "FullViewer",
            full_pub,
            AccessLevel::Full,
        ));
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-ro-viewer",
            "ReadOnlyViewer",
            ro_pub,
            AccessLevel::ReadOnly,
        ));
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-iso-viewer",
            "IsolatedViewer",
            iso_pub,
            AccessLevel::AgentIsolated,
        ));
    }

    let app = routes::create_router_with_frontend(state, None);

    // Open real SSE event streams as ReadOnly and AgentIsolated peers: neither may pin awake.
    let ro_tok =
        sign_request_with_origin(&ro_priv, "peer-ro-viewer", AuthenticatedOrigin::Owner).unwrap();
    let ro_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events/stream?scope=peer")
                .header("authorization", format!("Bearer {ro_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ro_stream.status(), StatusCode::OK);

    let iso_tok =
        sign_request_with_origin(&iso_priv, "peer-iso-viewer", AuthenticatedOrigin::Owner).unwrap();
    let iso_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events/stream?scope=peer")
                .header("authorization", format!("Bearer {iso_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(iso_stream.status(), StatusCode::OK);

    let (idle_status, idle_body) = request_json(
        app.clone(),
        "GET",
        "/api/mesh/cloud/peer/idle",
        "owner-secret",
    )
    .await;
    assert_eq!(idle_status, StatusCode::OK, "{idle_body}");
    assert_eq!(
        idle_body["attached_viewers"], 0,
        "ReadOnly and AgentIsolated SSE streams must not count as attached_viewers: {idle_body}"
    );
    assert_eq!(idle_body["idle"], true);

    // Drain succeeds despite open ReadOnly and AgentIsolated streams.
    let (drain_status, drain_body) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/drain",
        "owner-secret",
    )
    .await;
    assert_eq!(drain_status, StatusCode::OK, "{drain_body}");
    let (reopen_status, _) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/reopen",
        "owner-secret",
    )
    .await;
    assert_eq!(reopen_status, StatusCode::OK);

    // Opening an SSE stream as a Full peer counts as an authorized viewer and blocks drain.
    let full_tok =
        sign_request_with_origin(&full_priv, "peer-full-viewer", AuthenticatedOrigin::Owner)
            .unwrap();
    let full_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events/stream?scope=peer")
                .header("authorization", format!("Bearer {full_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(full_stream.status(), StatusCode::OK);

    let (_, busy_body) = request_json(
        app.clone(),
        "GET",
        "/api/mesh/cloud/peer/idle",
        "owner-secret",
    )
    .await;
    assert_eq!(busy_body["attached_viewers"], 1, "{busy_body}");
    assert_eq!(busy_body["idle"], false);

    let (blocked_drain, blocked_body) = request_json(
        app.clone(),
        "POST",
        "/api/mesh/cloud/peer/drain",
        "owner-secret",
    )
    .await;
    assert_eq!(blocked_drain, StatusCode::CONFLICT, "{blocked_body}");
    assert_eq!(blocked_body["error"]["code"], "machine_busy");

    // Closing the Full peer stream drops authorized viewers back to 0 even with ro/iso still open.
    drop(full_stream);
    let (_, after_full_drop) = request_json(
        app.clone(),
        "GET",
        "/api/mesh/cloud/peer/idle",
        "owner-secret",
    )
    .await;
    assert_eq!(after_full_drop["attached_viewers"], 0, "{after_full_drop}");
    assert_eq!(after_full_drop["idle"], true);

    // Local owner SSE stream also counts as an authorized viewer.
    let owner_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/events/stream")
                .header("authorization", "Bearer owner-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_stream.status(), StatusCode::OK);

    let (_, owner_busy) =
        request_json(app, "GET", "/api/mesh/cloud/peer/idle", "owner-secret").await;
    assert_eq!(owner_busy["attached_viewers"], 1, "{owner_busy}");
    assert_eq!(owner_busy["idle"], false);

    drop((owner_stream, ro_stream, iso_stream));
}
