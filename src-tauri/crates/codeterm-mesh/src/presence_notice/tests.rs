use super::*;
use crate::{
    account_mesh::{AccountMeshState, AccountPresence},
    types::{AccessLevel, MeshIdentity, PairedPeer, PeerAddress, RELAY_PROTOCOL_VERSION},
};

fn peer(id: &str, online: bool) -> PairedPeer {
    PairedPeer {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: vec![PeerAddress::from_ip("100.64.0.2")],
        api_port: 7685,
        public_key: "fixture".into(),
        paired_at: "fixture".into(),
        last_seen: None,
        online,
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
fn presence_notice_targets_retained_online_peers_without_reopening_local_traffic() {
    let mut manager = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "sender".into(),
            private_key: None,
            public_key: None,
        },
        "Sender".into(),
        7685,
    );
    for peer in [
        peer("online", true),
        peer("offline", false),
        peer("blocked", true),
    ] {
        manager.insert_legacy_pair_for_test(peer);
    }
    manager.revocation_list.blocked.push("blocked".into());
    manager.apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedOut,
        ..Default::default()
    });
    assert!(!manager.traffic_open("online"));
    let targets = notice_targets(&manager, false);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0[0].ip, "100.64.0.2");
    assert!(!manager.traffic_open("online"));
}

#[test]
fn registry_invalidation_targets_retained_peers_and_heartbeat_carries_observed_revision() {
    let mut manager = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "sender".into(),
            private_key: None,
            public_key: None,
        },
        "Sender".into(),
        7685,
    );
    for peer in [
        peer("online", true),
        peer("offline", false),
        peer("blocked", true),
        peer("removed", true),
    ] {
        manager.insert_legacy_pair_for_test(peer);
    }
    manager.revocation_list.blocked.push("blocked".into());
    manager.revocation_list.revoked.push("removed".into());
    assert_eq!(notice_targets(&manager, true).len(), 2);
    manager.apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedIn {
            account_id: "account".into(),
        },
        ..Default::default()
    });
    let revision = crate::types::RegistryRevision {
        account_id: "account".into(),
        revision: 42,
        security_digest: "security-digest".into(),
    };
    manager.registry_revision = Some(revision.clone());
    let value = serde_json::to_value(manager.build_announce()).unwrap();
    let announce: crate::types::PeerAnnounce = serde_json::from_value(value).unwrap();
    assert_eq!(announce.registry, Some(revision));
    manager.apply_account_state(AccountMeshState {
        presence: AccountPresence::SignedOut,
        ..Default::default()
    });
    assert!(manager.build_announce().registry.is_none());
}
