use super::*;
use codeterm_service_core::mesh::{
    account_mesh::AccountPresence,
    types::{AccessLevel, MeshIdentity, PairedPeer, RELAY_PROTOCOL_VERSION},
};

fn peer(id: &str, public: String) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: "Sender".into(),
        addresses: vec![],
        api_port: 7685,
        public_key: public,
        paired_at: "fixture".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::ReadOnly,
        version: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[test]
fn presence_notice_requires_a_signed_in_receiver_and_paired_nonreplayed_signature() {
    let (private, public) = crown::generate_peer_keypair();
    let mut mesh = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "receiver".into(),
            private_key: None,
            public_key: None,
        },
        "Receiver".into(),
        7685,
    );
    mesh.insert_account_peer_for_test(peer("sender", public));
    let admitted = mesh.account_state().clone();
    let mut signed_out = admitted.clone();
    signed_out.presence = AccountPresence::SignedOut;
    signed_out.membership = None;
    mesh.apply_account_state(signed_out);
    let token = crown::sign_request(&private, "sender").unwrap();
    assert_eq!(
        authorize_notice(&mut mesh, &token).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    mesh.apply_account_state(admitted);
    assert_eq!(
        authorize_notice(&mut mesh, "forged").unwrap_err().0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(authorize_notice(&mut mesh, &token).unwrap(), "sender");
    assert_eq!(
        authorize_notice(&mut mesh, &token).unwrap_err().0,
        StatusCode::UNAUTHORIZED
    );
    let fresh = crown::sign_request(&private, "sender").unwrap();
    assert_eq!(authorize_notice(&mut mesh, &fresh).unwrap(), "sender");
    let unpaired = crown::sign_request(&private, "unpaired").unwrap();
    assert_eq!(
        authorize_notice(&mut mesh, &unpaired).unwrap_err().0,
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn presence_notice_cannot_carry_state_or_traffic_instructions() {
    assert!(
        serde_json::from_value::<PresenceNotice>(serde_json::json!({"metadata_revision":1}))
            .is_ok()
    );
    for value in [
        serde_json::json!({"metadata_revision":1,"presence":"signed_out"}),
        serde_json::json!({"metadata_revision":1,"endpoint":"untrusted"}),
        serde_json::json!({"metadata_revision":-1}),
    ] {
        assert!(serde_json::from_value::<PresenceNotice>(value).is_err());
    }
}

#[test]
fn account_registry_block_allows_signed_notice_then_reopens_only_after_observation() {
    let (private, public) = crown::generate_peer_keypair();
    let sender = "00000000-0000-4000-8000-000000000001";
    let mut mesh = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "receiver".into(),
            private_key: None,
            public_key: None,
        },
        "Receiver".into(),
        7685,
    );
    mesh.set_clock(std::sync::Arc::new(|| 100));
    mesh.insert_account_peer_for_test(peer(sender, public));
    let mut state = mesh.account_state().clone();
    assert!(mesh.traffic_open(sender));
    state.unavailable_installations.insert(sender.into());
    mesh.apply_account_state(state.clone());
    assert!(!mesh.traffic_open(sender));
    assert!(!mesh.account_peer_revoked(sender));
    let notice = crown::sign_request(&private, sender).unwrap();
    assert_eq!(authorize_notice(&mut mesh, &notice).unwrap(), sender);
    assert!(!mesh.traffic_open(sender));
    state.unavailable_installations.clear();
    mesh.apply_account_state(state);
    assert!(mesh.traffic_open(sender));
}
