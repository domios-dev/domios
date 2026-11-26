use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    mesh::{
        crown::{generate_peer_keypair, sign_request_with_origin, AuthenticatedOrigin},
        types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION},
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
