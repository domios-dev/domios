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
            CloudCheckoutSession, CloudError, CloudMachineController, CloudMachineStore,
            CloudPlanAllowances, CloudPromoRedemption, CloudProvisioningPort, CloudSleepMode,
            CloudUsagePack, CloudUsageSize, CloudUsageSummary, ProvisionedLease,
            ProvisionedMachine, ProvisioningCreateRequest, SystemClock,
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
    state
        .svc
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            CloudMachineStore::new(fixture.temp.path().join("cloud_machines.json")),
            "test-peer-auth".into(),
            Arc::new(SystemClock),
        )));
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
    state
        .svc
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            CloudMachineStore::new(fixture.temp.path().join("cloud_machines.json")),
            "test-peer-viewers".into(),
            Arc::new(SystemClock),
        )));
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

struct FakeCheckoutProvisioning {
    seen_requests: Mutex<Vec<(String, Option<u64>)>>,
    outcomes: Mutex<VecDeque<Result<CloudCheckoutSession, CloudError>>>,
    usage: CloudUsageSummary,
}

impl FakeCheckoutProvisioning {
    fn new(
        outcomes: Vec<Result<CloudCheckoutSession, CloudError>>,
        usage: CloudUsageSummary,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen_requests: Mutex::new(Vec::new()),
            outcomes: Mutex::new(outcomes.into()),
            usage,
        })
    }
}

#[async_trait]
impl CloudProvisioningPort for FakeCheckoutProvisioning {
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
        Ok(self.usage.clone())
    }

    async fn create_checkout(
        &self,
        pack_id: &str,
        amount_cents: Option<u64>,
    ) -> Result<CloudCheckoutSession, CloudError> {
        self.seen_requests
            .lock()
            .push((pack_id.to_owned(), amount_cents));
        self.outcomes
            .lock()
            .pop_front()
            .unwrap_or(Err(CloudError::CheckoutUnavailable("exhausted".into())))
    }
}

async fn post_checkout(app: axum::Router, token: &str, payload: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mesh/cloud/checkout")
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
async fn cloud_checkout_route_enforces_local_owner_authority_and_maps_pack_custom_and_refusals() {
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
    let (ro_priv, ro_pub) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-full-checkout",
            "FullPeer",
            full_pub,
            AccessLevel::Full,
        ));
        mesh.insert_account_peer_for_test(paired_peer_with_access(
            "peer-ro-checkout",
            "ReadOnlyPeer",
            ro_pub,
            AccessLevel::ReadOnly,
        ));
    }

    let provisioning = FakeCheckoutProvisioning::new(
        vec![
            Ok(CloudCheckoutSession {
                url: "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5".into(),
                session_id: "cs_test_fixture_pack_5".into(),
                pack_id: "pack_5".into(),
                credits: 25.0,
                amount_cents: 500,
                currency: "USD".into(),
            }),
            Ok(CloudCheckoutSession {
                url: "https://checkout.stripe.com/c/pay/cs_test_fixture_custom".into(),
                session_id: "cs_test_fixture_custom".into(),
                pack_id: "custom".into(),
                credits: 35.0,
                amount_cents: 700,
                currency: "USD".into(),
            }),
            Err(CloudError::PromoSpendCapExceeded),
            Err(CloudError::CheckoutBelowMin),
            Err(CloudError::CheckoutAboveMax),
            Err(CloudError::CheckoutTerritory),
            Err(CloudError::CheckoutInvalid),
            Err(CloudError::CheckoutUnavailable(
                "stripe unconfigured".into(),
            )),
            Err(CloudError::RateLimited {
                retry_after_seconds: 90,
            }),
        ],
        CloudUsageSummary::default(),
    );
    assert!(state
        .svc
        .set_ext(CloudProvisioningOverride(provisioning.clone()))
        .is_ok());

    let app = routes::create_router_with_frontend(state, None);

    // Local agent tab is refused (local owner only).
    let (agent_status, agent_body) =
        post_checkout(app.clone(), &agent_token, json!({ "pack_id": "pack_5" })).await;
    assert_eq!(agent_status, StatusCode::FORBIDDEN, "{agent_body}");
    assert_eq!(agent_body["error"]["code"], "local_owner_only");

    // Remote peers (Full owner, Full agent, ReadOnly) are refused.
    let full_owner_tok =
        sign_request_with_origin(&full_priv, "peer-full-checkout", AuthenticatedOrigin::Owner)
            .unwrap();
    let (peer_owner_status, _) =
        post_checkout(app.clone(), &full_owner_tok, json!({ "pack_id": "pack_5" })).await;
    assert_eq!(peer_owner_status, StatusCode::FORBIDDEN);

    let full_agent = AuthenticatedAgent {
        instance_id: "peer-full-checkout".into(),
        tab_id: "remote-tab".into(),
        can_spawn: false,
    };
    let full_agent_tok = sign_request_with_agent(
        &full_priv,
        "peer-full-checkout",
        AuthenticatedOrigin::Agent,
        Some(&full_agent),
    )
    .unwrap();
    let (peer_agent_status, _) =
        post_checkout(app.clone(), &full_agent_tok, json!({ "pack_id": "pack_5" })).await;
    assert_eq!(peer_agent_status, StatusCode::FORBIDDEN);

    let ro_tok =
        sign_request_with_origin(&ro_priv, "peer-ro-checkout", AuthenticatedOrigin::Owner).unwrap();
    let (ro_status, _) = post_checkout(app.clone(), &ro_tok, json!({ "pack_id": "pack_5" })).await;
    assert_eq!(ro_status, StatusCode::FORBIDDEN);
    assert!(provisioning.seen_requests.lock().is_empty());

    // Structural validation rejects blank pack_id, malformed custom, extra amount_cents, and unknown fields.
    for bad in [
        json!({ "pack_id": "   " }),
        json!({ "pack_id": "custom" }),
        json!({ "pack_id": "custom", "amount_cents": 0 }),
        json!({ "pack_id": "pack_5", "amount_cents": 500 }),
    ] {
        let (st, body) = post_checkout(app.clone(), "owner-secret", bad).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid");
    }
    let (extra_status, _) = post_checkout(
        app.clone(),
        "owner-secret",
        json!({ "pack_id": "pack_5", "unknown": true }),
    )
    .await;
    assert_eq!(extra_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(provisioning.seen_requests.lock().is_empty());

    // Local owner preset pack checkout forwards cleanly.
    let (pack_status, pack_body) = post_checkout(
        app.clone(),
        "owner-secret",
        json!({ "pack_id": " pack_5 " }),
    )
    .await;
    assert_eq!(pack_status, StatusCode::OK, "{pack_body}");
    assert_eq!(
        pack_body["url"],
        "https://checkout.stripe.com/c/pay/cs_test_fixture_pack_5"
    );
    assert_eq!(pack_body["session_id"], "cs_test_fixture_pack_5");
    assert_eq!(pack_body["pack_id"], "pack_5");
    assert_eq!(pack_body["credits"], 25.0);
    assert_eq!(pack_body["amount_cents"], 500);
    assert_eq!(pack_body["currency"], "USD");

    // Local owner custom checkout forwards pack_id="custom" and amount_cents.
    let (custom_status, custom_body) = post_checkout(
        app.clone(),
        "owner-secret",
        json!({ "pack_id": "custom", "amount_cents": 700 }),
    )
    .await;
    assert_eq!(custom_status, StatusCode::OK, "{custom_body}");
    assert_eq!(
        custom_body["url"],
        "https://checkout.stripe.com/c/pay/cs_test_fixture_custom"
    );
    assert_eq!(custom_body["pack_id"], "custom");
    assert_eq!(custom_body["amount_cents"], 700);
    assert_eq!(
        provisioning.seen_requests.lock().as_slice(),
        &[("pack_5".into(), None), ("custom".into(), Some(700))]
    );

    // Typed refusals map to plain codes and expected statuses.
    for (expected_status, expected_code) in [
        (StatusCode::CONFLICT, "spend_cap_exceeded"),
        (StatusCode::BAD_REQUEST, "below_min"),
        (StatusCode::BAD_REQUEST, "above_max"),
        (StatusCode::FORBIDDEN, "territory"),
        (StatusCode::BAD_REQUEST, "invalid"),
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    ] {
        let (st, body) =
            post_checkout(app.clone(), "owner-secret", json!({ "pack_id": "pack_5" })).await;
        assert_eq!(st, expected_status, "{body}");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], expected_code);
        assert!(body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()));
    }

    let (rl_status, rl_body) =
        post_checkout(app, "owner-secret", json!({ "pack_id": "pack_5" })).await;
    assert_eq!(rl_status, StatusCode::TOO_MANY_REQUESTS, "{rl_body}");
    assert_eq!(rl_body["error"]["code"], "rate_limited");
    assert_eq!(rl_body["error"]["retry_after_seconds"], 90);
}

#[test]
fn checkout_account_refusal_mapping_preserves_plain_codes() {
    use codeterm_account::account_mesh::AccountMeshRefusal;

    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::PromoSpendCapExceeded).code(),
        "spend_cap_exceeded"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::CheckoutBelowMin).code(),
        "below_min"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::CheckoutAboveMax).code(),
        "above_max"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::CheckoutTerritory).code(),
        "territory"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::Invalid).code(),
        "invalid"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::Unavailable).code(),
        "unavailable"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::Offline).code(),
        "unavailable"
    );
    assert_eq!(
        super::adapters::map_checkout_refusal(AccountMeshRefusal::RateLimited {
            retry_after_seconds: 30
        }),
        CloudError::RateLimited {
            retry_after_seconds: 30
        }
    );
}

#[tokio::test]
async fn cloud_usage_route_forwards_custom_bounds_and_full_catalog_fields() {
    crate::register_api_extensions();
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let mut burn = std::collections::BTreeMap::new();
    burn.insert("small".into(), 1.0);
    let usage = CloudUsageSummary {
        warnings: None,
        used: 1.5,
        allowance: 10.0,
        purchased: 25.0,
        remaining: 33.5,
        cap: 50.0,
        burn_per_size: burn,
        currency: Some("USD".into()),
        period_end_at: Some(1_798_761_600),
        custom_min_cents: Some(500),
        custom_max_cents: Some(4500),
        plan: Some("pro".into()),
        sizes: Some(vec![CloudUsageSize {
            id: "small".into(),
            cpu: 2,
            ram_gb: 4.0,
            hourly_rate_cents: 20,
            credits_per_hour: 1.0,
        }]),
        packs: Some(vec![
            CloudUsagePack {
                pack_id: "pack_5".into(),
                price_cents: 500,
                hours: 25.0,
            },
            CloudUsagePack {
                pack_id: "pack_10".into(),
                price_cents: 1000,
                hours: 50.0,
            },
        ]),
        purchased_expiry_days: Some(365),
        allowance_hours: Some(CloudPlanAllowances {
            free: 0.0,
            pro: 10.0,
            max: 40.0,
            sponsor: 120.0,
        }),
        storage: None,
    };
    let provisioning = FakeCheckoutProvisioning::new(vec![], usage);
    assert!(state
        .svc
        .set_ext(CloudProvisioningOverride(provisioning))
        .is_ok());

    let app = routes::create_router_with_frontend(state, None);
    let (status, body) = request_json(app, "GET", "/api/mesh/cloud/usage", "owner-secret").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["used"], 1.5);
    assert_eq!(body["allowance"], 10.0);
    assert_eq!(body["purchased"], 25.0);
    assert_eq!(body["remaining"], 33.5);
    assert_eq!(body["cap"], 50.0);
    assert_eq!(body["burn_per_size"]["small"], 1.0);
    assert_eq!(body["currency"], "USD");
    assert_eq!(body["period_end_at"], 1_798_761_600u64);
    assert_eq!(body["custom_min_cents"], 500);
    assert_eq!(body["custom_max_cents"], 4500);
    assert_eq!(body["plan"], "pro");
    assert_eq!(body["sizes"][0]["id"], "small");
    assert_eq!(body["sizes"][0]["cpu"], 2);
    assert_eq!(body["sizes"][0]["ram_gb"], 4.0);
    assert_eq!(body["sizes"][0]["hourly_rate_cents"], 20);
    assert_eq!(body["sizes"][0]["credits_per_hour"], 1.0);
    assert_eq!(body["packs"][0]["pack_id"], "pack_5");
    assert_eq!(body["packs"][0]["price_cents"], 500);
    assert_eq!(body["packs"][0]["hours"], 25.0);
    assert_eq!(body["purchased_expiry_days"], 365);
    assert_eq!(body["allowance_hours"]["free"], 0.0);
    assert_eq!(body["allowance_hours"]["pro"], 10.0);
    assert_eq!(body["allowance_hours"]["max"], 40.0);
    assert_eq!(body["allowance_hours"]["sponsor"], 120.0);
}
