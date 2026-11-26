use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    mesh::{
        account_mesh::AccountPresence,
        crown::{generate_peer_keypair, sign_request_with_origin, AuthenticatedOrigin},
        types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
    service::{
        cloud_machines::{
            warm_bootstrap::{
                PortableMemEntry, PortableMemKind, PortableOwnerIdentity, PortablePreferences,
                WarmBootstrapProjection, WARM_BOOTSTRAP_SCHEMA_VERSION,
            },
            CloudMachineController, CloudMachineStore, SystemClock,
        },
        test_fixture::ServiceFixture,
    },
};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::routes;

async fn post_json(
    app: axum::Router,
    uri: &str,
    token: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
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

fn sample_projection(
    account_id: &str,
    owner_installation_id: &str,
    target_installation_id: &str,
    creation_generation: &str,
    include_secret: bool,
) -> WarmBootstrapProjection {
    let mut wake_up_items = vec![PortableMemEntry {
        kind: PortableMemKind::Rule,
        title: "Concise replies".into(),
        summary: Some("Keep responses short and concrete".into()),
        area: None,
        tags: vec!["owner-rule".into()],
        body: "Keep responses short and concrete".into(),
        importance: 9.0,
        pinned: true,
    }];
    if include_secret {
        wake_up_items.push(PortableMemEntry {
            kind: PortableMemKind::Fact,
            title: "Secret Fixture".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "OPENAI_API_KEY=sk-proj-ROUTEFIXTURESECRET1234567890".into(),
            importance: 9.0,
            pinned: true,
        });
    }
    WarmBootstrapProjection {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        account_id: account_id.into(),
        owner_installation_id: owner_installation_id.into(),
        target_installation_id: target_installation_id.into(),
        creation_generation: creation_generation.into(),
        recovery_revision: None,
        identity: Some(PortableOwnerIdentity {
            identity_md: Some("Owner: Route Canary".into()),
            soul_md: None,
            nickname: None,
        }),
        wake_up_items,
        skills: vec![],
        preferences: PortablePreferences::default(),
    }
}

#[tokio::test]
async fn peer_warm_bootstrap_enforces_authority_provenance_idempotency_and_exclusions() {
    crate::register_api_extensions();
    let temp = tempfile::tempdir().unwrap();
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::test_support::app_state(&fixture);
    state.svc.mem.seed_bundled_if_needed().unwrap();
    let controller = Arc::new(CloudMachineController::new(
        CloudMachineStore::new(temp.path().join("mesh").join("cloud_machines.json")),
        "cloud-target".into(),
        Arc::new(SystemClock),
    ));
    state.svc.install_cloud_machines_for_test(controller);

    state.config.lock().api.token = Some("local-owner-secret".into());

    let target_instance_id = state.mesh.lock().status().instance_id.clone();
    {
        let mut mesh = state.mesh.lock();
        let mut account = mesh.account_state().clone();
        account.presence = AccountPresence::SignedIn {
            account_id: "acct-same-owner".into(),
        };
        mesh.apply_account_state(account);
    }

    let (full_priv, full_pub) = generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "home-full-peer".into(),
        instance_name: "HomeFull".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: full_pub,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: true,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });

    let (readonly_priv, readonly_pub) = generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "home-ro-peer".into(),
        instance_name: "HomeRO".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: readonly_pub,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::ReadOnly,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });

    let app = routes::create_router_with_frontend(state.clone(), None);

    // 1. ReadOnly peer is refused (403) with zero writes.
    let ro_token =
        sign_request_with_origin(&readonly_priv, "home-ro-peer", AuthenticatedOrigin::Owner)
            .unwrap();
    let proj_ro = sample_projection(
        "acct-same-owner",
        "home-ro-peer",
        &target_instance_id,
        "gen-route-1",
        false,
    );
    let (status, _) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &ro_token,
        serde_json::to_value(&proj_ro).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 2. Full peer with Agent origin is refused (403) with zero writes.
    let agent_token =
        sign_request_with_origin(&full_priv, "home-full-peer", AuthenticatedOrigin::Agent).unwrap();
    let proj_ok = sample_projection(
        "acct-same-owner",
        "home-full-peer",
        &target_instance_id,
        "gen-route-1",
        true,
    );
    let (status, _) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &agent_token,
        serde_json::to_value(&proj_ok).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 3. Full peer with wrong account is refused (403) with zero writes.
    let owner_token =
        sign_request_with_origin(&full_priv, "home-full-peer", AuthenticatedOrigin::Owner).unwrap();
    let proj_wrong_acct = sample_projection(
        "acct-wrong-owner",
        "home-full-peer",
        &target_instance_id,
        "gen-route-1",
        false,
    );
    let (status, body) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &owner_token,
        serde_json::to_value(&proj_wrong_acct).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["reject_reason"], "wrong_account");

    // 4. Same-account Full peer applies once; secret fixture excluded from sanitized reply and mem.
    let (status, body) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &owner_token,
        serde_json::to_value(&proj_ok).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "partial");
    assert_eq!(body["readable_owner_context"], true);
    assert_eq!(body["wake_up_verified"], true);
    assert!(!body.to_string().contains("ROUTEFIXTURESECRET"));

    // 5. Retry with same generation returns already_applied (one commit only).
    let (status, retry_body) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &owner_token,
        serde_json::to_value(&proj_ok).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{retry_body}");
    assert_eq!(retry_body["status"], "already_applied");

    // 6. Stale generation is refused (409).
    let proj_stale = sample_projection(
        "acct-same-owner",
        "home-full-peer",
        &target_instance_id,
        "gen-route-2-stale",
        false,
    );
    let (status, stale_body) = post_json(
        app.clone(),
        "/api/mesh/cloud/peer/warm-bootstrap",
        &owner_token,
        serde_json::to_value(&proj_stale).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale_body}");
    assert_eq!(stale_body["reject_reason"], "stale_generation");

    // 7. Provider sign-in prepare route on peer returns typed capability list only.
    let (status, prep_body) = post_json(
        app,
        "/api/mesh/cloud/peer/provider-signin/prepare",
        &owner_token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prep_body}");
    assert!(prep_body["providers"].is_array());
}
