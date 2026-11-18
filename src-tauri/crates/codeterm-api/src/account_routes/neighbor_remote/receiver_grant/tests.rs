use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::post,
    Router,
};
use codeterm_service_core::{
    mesh::{
        account_mesh::{AccountMeshState, AccountPresence, SignedMembership, VerifiedMembership},
        crown::{generate_peer_keypair, sign_request, sign_request_with_origin},
        types::{PairedPeer, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

struct Credentials;

#[async_trait::async_trait]
impl codeterm_account::CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(Some(codeterm_account::Secret::new("fake-refresh")))
    }
    async fn save(&self, _: &str, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}

fn pair(state: &AppState, uid: &str, public: &str) {
    let mut mesh = state.mesh.lock();
    let expires_at = mesh.now() + 3600;
    mesh.apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: uid.into(),
        },
        membership: Some(SignedMembership {
            token: "opaque-membership".into(),
            verified: VerifiedMembership {
                account_id: uid.into(),
                installation_id: "source".into(),
                key_id: "fixture-key".into(),
                key_generation: 1,
                slot_revision: 1,
                issued_at: 0,
                expires_at,
            },
        }),
        ..Default::default()
    });
    mesh.insert_account_peer_for_test(PairedPeer {
        instance_id: "target".into(),
        instance_name: "Target".into(),
        addresses: vec![],
        api_port: 0,
        public_key: public.into(),
        paired_at: "fixture".into(),
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
    assert!(mesh.traffic_open("target"));
}

fn request(token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/neighbors/runtime")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("x-codeterm-authenticated-origin", "Owner")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn account(state: &AppState) {
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(codeterm_account::adapters::fake::Fake::default()),
        Arc::new(Credentials),
        std::time::Duration::from_secs(1),
    ));
    account.refresh().await.unwrap();
    assert!(state
        .svc
        .ext::<codeterm_neighbors_service::service::account::AccountCell>()
        .0
        .set(account)
        .is_ok());
}

#[tokio::test]
async fn receiver_grant_mesh_origin_reads_the_previous_receivers_persisted_full_projection() {
    let fixture = codeterm_neighbors_service::test_support::fixture(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    account(&state).await;
    state.config.lock().mesh.enabled = true;
    let (key, public) = generate_peer_keypair();
    pair(&state, "fake-user", &public);
    let friendship = "d".repeat(64);
    let metadata = codeterm_neighbors_service::service::neighbors::storage::Metadata::new(
        state.svc.account_data_root().unwrap(),
        "fake-user",
    );
    let mut settings = metadata.settings().await.unwrap();
    settings.permissions.insert(
        friendship.clone(),
        serde_json::from_value(json!({
            "allowed":true,"machine_access":"full","scope":"Share the joint project"
        }))
        .unwrap(),
    );
    metadata.save_settings(settings).await.unwrap();
    let expected = state
        .svc
        .neighbor_runtime(
            NeighborRuntimeCommand::Permission {
                friendship_id: friendship.clone(),
            },
            None,
            None,
        )
        .await
        .unwrap();
    let app = Router::new()
        .route("/api/neighbors/runtime", post(super::super::runtime))
        .with_state(state.clone());
    let token = sign_request(&key, "target").unwrap();
    let response = app
        .clone()
        .oneshot(request(
            &token,
            json!({"operation":"permission","friendship_id":friendship}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let projection: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(projection, expected);
    assert_eq!(projection["machine_access"], "full");
    assert_eq!(projection["folder_enforcement"]["writes"], "unrestricted");
    for body in [
        json!({"operation":"status"}),
        json!({"operation":"history","friendship_id":friendship}),
        json!({"operation":"set_permission","friendship_id":friendship,"permission":{}}),
        json!({"operation":"rotate"}),
        json!({"operation":"set_friend_receiver","friendship_id":friendship,"receiver_operation":"set","expected_receiver_revision":4,"takeover_confirmed":true}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(&token, body))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        json!(metadata.settings().await.unwrap().permissions[&friendship].machine_access),
        "full"
    );
}

#[tokio::test]
async fn receiver_grant_refuses_foreign_account_read_only_revoked_and_forged_origins() {
    let fixture = codeterm_neighbors_service::test_support::fixture(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    account(&state).await;
    state.config.lock().mesh.enabled = true;
    let (key, public) = generate_peer_keypair();
    let body = json!({"operation":"permission","friendship_id":"d".repeat(64)});
    let app = Router::new()
        .route("/api/neighbors/runtime", post(super::super::runtime))
        .with_state(state.clone());
    pair(&state, "other-account", &public);
    let token = sign_request(&key, "target").unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(&token, body.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    pair(&state, "fake-user", &public);
    state
        .mesh
        .lock()
        .paired_peers
        .get_mut("target")
        .unwrap()
        .access_level = AccessLevel::ReadOnly;
    assert_eq!(
        app.clone()
            .oneshot(request(&token, body.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    pair(&state, "fake-user", &public);
    for origin in [
        crate::auth::AuthenticatedOrigin::Mesh,
        crate::auth::AuthenticatedOrigin::Agent,
    ] {
        let (unknown_key, _) = generate_peer_keypair();
        let forged = sign_request_with_origin(&unknown_key, "target", origin).unwrap();
        assert_eq!(
            app.clone()
                .oneshot(request(&forged, body.clone()))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let mut revoked = state.mesh.lock().account_state().clone();
    revoked.revoked_installations.insert("target".into());
    state.mesh.lock().apply_account_state(revoked);
    assert_eq!(
        app.clone()
            .oneshot(request(&token, body.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    pair(&state, "fake-user", &public);
    state.config.lock().mesh.enabled = false;
    assert_eq!(
        app.oneshot(request(&token, body)).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}
