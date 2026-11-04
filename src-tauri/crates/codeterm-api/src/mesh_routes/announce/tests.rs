use super::*;
use codeterm_service_core::mesh::{
    account_mesh::{
        protocol::PartyDescriptor, transcript::Party, AccountMeshAuthority, AccountMeshState,
        AccountPresence, AdmissionRefusal, BoxFuture, LinkTarget, SignedMembership,
        VerifiedMembership,
    },
    types::{PeerAddress, PeerAnnounce, RELAY_PROTOCOL_VERSION},
};
use std::sync::Arc;

const PEER: &str = "00000000-0000-4000-8000-000000000001";

fn membership(installation: &str, expires_at: u64) -> SignedMembership {
    SignedMembership {
        token: "fixture".into(),
        verified: VerifiedMembership {
            account_id: "account".into(),
            installation_id: installation.into(),
            key_id: "key".into(),
            key_generation: 1,
            slot_revision: 1,
            issued_at: 100,
            expires_at,
        },
    }
}

struct Authority;
impl AccountMeshAuthority for Authority {
    fn verify_membership(
        &self,
        token: &str,
        _now: u64,
    ) -> Result<VerifiedMembership, AdmissionRefusal> {
        let mut verified = membership(
            "peer-installation",
            if token == "renewed-again" { 600 } else { 500 },
        )
        .verified;
        verified.slot_revision = 2;
        Ok(verified)
    }
    fn sign_possession(&self, _: &[u8; 32]) -> Result<String, AdmissionRefusal> {
        Err(AdmissionRefusal::Offline)
    }
    fn verify_possession(&self, _: &VerifiedMembership, _: &[u8; 32], _: &str) -> bool {
        false
    }
    fn resolve_link_target(&self, _: &str) -> BoxFuture<'_, Result<LinkTarget, AdmissionRefusal>> {
        Box::pin(async { Err(AdmissionRefusal::Offline) })
    }
}

#[tokio::test]
async fn announce_offline_routes_and_refreshes_membership_but_rejects_replay() {
    use axum::response::IntoResponse;
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    crate::test_support::install_app_version();
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let (private, public) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.set_clock(Arc::new(|| 100));
        mesh.bind_account_authority(Arc::new(Authority));
        mesh.apply_account_state(AccountMeshState {
            registry_members: Some(
                [
                    ("local-installation".into(), ("key".into(), 1, 1)),
                    ("peer-installation".into(), ("key".into(), 1, 2)),
                ]
                .into(),
            ),
            presence: AccountPresence::SignedIn {
                account_id: "account".into(),
            },
            membership: Some(membership("local-installation", 1000)),
            ..Default::default()
        });
        mesh.admit_new_link(
            &PartyDescriptor {
                party: Party {
                    instance_id: PEER.into(),
                    mesh_public_key: public,
                    installation_id: "peer-installation".into(),
                },
                instance_name: "Peer".into(),
                api_port: 7685,
                addresses: vec![],
            },
            &[],
            membership("peer-installation", 120),
            "account",
            "link",
        )
        .unwrap();
    }
    let token = codeterm_service_core::mesh::crown::sign_request(&private, PEER).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers.insert(
        codeterm_service_core::mesh::version::VERSION_HEADER,
        codeterm_service_core::mesh::version::local_version()
            .parse()
            .unwrap(),
    );
    headers.insert(
        codeterm_service_core::mesh::account_mesh::protocol::MEMBERSHIP_HEADER,
        "renewed".parse().unwrap(),
    );
    let announce = PeerAnnounce {
        instance_id: PEER.into(),
        instance_name: "Peer".into(),
        addresses: vec![PeerAddress::from_ip("192.0.2.9")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
    };
    let (health_started, pending_health) = tokio::sync::oneshot::channel();
    let (finish_health, held_response) = tokio::sync::oneshot::channel();
    let health = {
        let state = state.clone();
        tokio::spawn(async move {
            let response = crate::health_routes::health(AxumState(state)).await;
            health_started.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), held_response)
                .await
                .unwrap()
                .unwrap();
            response
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), pending_health)
        .await
        .unwrap()
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        mesh_announce_inbound(
            AxumState(state.clone()),
            headers.clone(),
            Json(announce.clone()),
        ),
    )
    .await
    .expect("announce completes without backend access")
    .unwrap()
    .into_response();
    finish_health.send(()).unwrap();
    let Json(health) = tokio::time::timeout(std::time::Duration::from_secs(5), health)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(health["startup"]["state"], "ready");
    assert_eq!(response.status(), StatusCode::OK);
    {
        let mesh = state.mesh.lock();
        assert_eq!(
            mesh.link_record(PEER)
                .unwrap()
                .remote_membership
                .as_ref()
                .unwrap()
                .verified
                .expires_at,
            500
        );
        assert!(mesh.paired_peers[PEER].addresses.is_empty());
        assert!(mesh.traffic_open(PEER));
    }
    let mut replay = announce.clone();
    replay.addresses = vec![PeerAddress::from_ip("192.0.2.10")];
    let retry = mesh_announce_inbound(AxumState(state.clone()), headers, Json(replay))
        .await
        .unwrap_err();
    assert_eq!(retry.0, StatusCode::UNAUTHORIZED);
    assert!(state.mesh.lock().paired_peers[PEER].addresses.is_empty());

    let events_before = fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(name, _)| name == "mesh-state-changed")
        .count();
    let token = codeterm_service_core::mesh::crown::sign_request(&private, PEER).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers.insert(
        codeterm_service_core::mesh::version::VERSION_HEADER,
        codeterm_service_core::mesh::version::local_version()
            .parse()
            .unwrap(),
    );
    headers.insert(
        codeterm_service_core::mesh::account_mesh::protocol::MEMBERSHIP_HEADER,
        "renewed-again".parse().unwrap(),
    );
    let announce = PeerAnnounce {
        instance_id: PEER.into(),
        instance_name: "Peer".into(),
        addresses: vec![PeerAddress::from_ip("192.0.2.9")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
    };
    let response = mesh_announce_inbound(
        AxumState(state.clone()),
        headers.clone(),
        Json(announce.clone()),
    )
    .await
    .unwrap();
    assert_eq!(response.0["changed"], true);
    assert_eq!(
        state
            .mesh
            .lock()
            .peer_membership_expiry("peer-installation"),
        Some(600)
    );
    assert_eq!(
        fixture
            .ui
            .0
            .lock()
            .iter()
            .filter(|(name, _)| name == "mesh-state-changed")
            .count(),
        events_before + 1
    );
    let duplicate = mesh_announce_inbound(
        AxumState(state.clone()),
        headers.clone(),
        Json(announce.clone()),
    )
    .await
    .unwrap_err();
    assert_eq!(duplicate.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        fixture
            .ui
            .0
            .lock()
            .iter()
            .filter(|(name, _)| name == "mesh-state-changed")
            .count(),
        events_before + 1
    );
    let fresh_token = codeterm_service_core::mesh::crown::sign_request(&private, PEER).unwrap();
    headers.insert(
        "authorization",
        format!("Bearer {fresh_token}").parse().unwrap(),
    );
    let mut spoofed = announce.clone();
    spoofed.instance_id = "other-peer".into();
    assert_eq!(
        mesh_announce_inbound(AxumState(state.clone()), headers.clone(), Json(spoofed))
            .await
            .unwrap_err()
            .0,
        StatusCode::FORBIDDEN
    );
    let mut unsupported = announce.clone();
    unsupported.relay_protocol_version = 1;
    assert_eq!(
        mesh_announce_inbound(AxumState(state.clone()), headers.clone(), Json(unsupported))
            .await
            .unwrap_err()
            .0,
        StatusCode::UPGRADE_REQUIRED
    );
    let mut unversioned = headers.clone();
    unversioned.remove(codeterm_service_core::mesh::version::VERSION_HEADER);
    assert_eq!(
        mesh_announce_inbound(
            AxumState(state.clone()),
            unversioned,
            Json(announce.clone())
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::UPGRADE_REQUIRED
    );
    let mut forged = headers.clone();
    forged.insert("authorization", "Bearer forged".parse().unwrap());
    assert_eq!(
        mesh_announce_inbound(AxumState(state.clone()), forged, Json(announce.clone()))
            .await
            .unwrap_err()
            .0,
        StatusCode::UNAUTHORIZED
    );
    let admitted = state.mesh.lock().account_state().clone();
    let mut revoked = admitted.clone();
    revoked
        .revoked_installations
        .insert("peer-installation".into());
    state.mesh.lock().apply_account_state(revoked);
    assert_eq!(
        mesh_announce_inbound(
            AxumState(state.clone()),
            headers.clone(),
            Json(announce.clone())
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    state.mesh.lock().apply_account_state(admitted);
    let mut signed_out = state.mesh.lock().account_state().clone();
    signed_out.presence = AccountPresence::SignedOut;
    signed_out.membership = None;
    state.mesh.lock().apply_account_state(signed_out);
    assert_eq!(
        mesh_announce_inbound(AxumState(state.clone()), headers, Json(announce))
            .await
            .unwrap_err()
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(state.mesh.lock().paired_peers[PEER].addresses.is_empty());
}
