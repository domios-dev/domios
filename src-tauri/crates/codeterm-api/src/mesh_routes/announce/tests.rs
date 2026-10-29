use super::*;
use codeterm_service_core::mesh::{
    account_mesh::{
        protocol::PartyDescriptor, transcript::Party, AccountMeshAuthority, AccountMeshState,
        AccountPresence, AdmissionRefusal, BoxFuture, LinkAuthorizationRequest,
        LinkPermitExpectation, LinkTarget, SignedMembership, VerifiedMembership,
    },
    manager::LinkOrigin,
    types::{PeerAddress, PeerAnnounce, RegistryRevision, RELAY_PROTOCOL_VERSION},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

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
        _token: &str,
        _now: u64,
    ) -> Result<VerifiedMembership, AdmissionRefusal> {
        let mut verified = membership("peer-installation", 500).verified;
        verified.slot_revision = 2;
        Ok(verified)
    }
    fn sign_possession(&self, _: &[u8; 32]) -> Result<String, AdmissionRefusal> {
        Err(AdmissionRefusal::Offline)
    }
    fn verify_possession(&self, _: &VerifiedMembership, _: &[u8; 32], _: &str) -> bool {
        false
    }
    fn authorize_link(
        &self,
        _: LinkAuthorizationRequest,
    ) -> BoxFuture<'_, Result<String, AdmissionRefusal>> {
        Box::pin(async { Err(AdmissionRefusal::Offline) })
    }
    fn verify_link_permit(
        &self,
        _: &str,
        _: &LinkPermitExpectation,
        _: u64,
    ) -> Result<(), AdmissionRefusal> {
        Err(AdmissionRefusal::Offline)
    }
    fn resolve_link_target(&self, _: &str) -> BoxFuture<'_, Result<LinkTarget, AdmissionRefusal>> {
        Box::pin(async { Err(AdmissionRefusal::Offline) })
    }
}

#[tokio::test]
async fn offline_registry_hint_never_delays_announce_or_membership_refresh_and_retries_skip_hint() {
    use axum::response::IntoResponse;
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let (private, public) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.set_clock(Arc::new(|| 100));
        mesh.bind_account_authority(Arc::new(Authority));
        mesh.apply_account_state(AccountMeshState {
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
            LinkOrigin::Remote,
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
        registry: Some(RegistryRevision {
            account_id: "account".into(),
            revision: 2,
            security_digest: "changed".into(),
        }),
        instance_id: PEER.into(),
        instance_name: "Peer".into(),
        addresses: vec![PeerAddress::from_ip("192.0.2.9")],
        api_port: 7685,
        online: true,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
    };
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let finished = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let hint = {
        let (entered, release, finished, calls) = (
            entered.clone(),
            release.clone(),
            finished.clone(),
            calls.clone(),
        );
        move |_, _, _| async move {
            calls.fetch_add(1, Ordering::SeqCst);
            entered.notify_one();
            release.notified().await;
            finished.notify_one();
            Err("Offline".into())
        }
    };
    let response =
        announce_with_registry_hint(state.clone(), headers.clone(), announce.clone(), hint)
            .await
            .unwrap()
            .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    entered.notified().await;
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
        assert_eq!(mesh.paired_peers[PEER].addresses[0].ip, "192.0.2.9");
        assert!(mesh.traffic_open(PEER));
    }
    release.notify_one();
    finished.notified().await;
    let retry = announce_with_registry_hint(state.clone(), headers, announce, |_, _, _| async {
        panic!("a repeated nonce must skip the hint")
    })
    .await
    .unwrap()
    .into_response();
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
