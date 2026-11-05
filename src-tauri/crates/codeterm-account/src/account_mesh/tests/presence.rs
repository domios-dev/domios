use super::support::*;
use crate::account_mesh::wire::{RegistryPresence, RegistryRefusal};
use crate::account_mesh::*;
use serde_json::json;

const UID: &str = "user-1";

#[test]
fn account_mesh_presence_wire_requires_an_explicit_known_value() {
    use wire::{MeshManagementRow, MeshOperation, MeshResult};
    let operation = MeshOperation::Presence {
        expected_metadata_revision: 3,
        presence: RegistryPresence::SignedOut,
    };
    assert_eq!(
        serde_json::to_value(operation).unwrap(),
        json!({
            "operation": "presence", "expected_metadata_revision": 3, "presence": "signed_out"
        })
    );
    let result: MeshResult = serde_json::from_value(json!({
        "kind": "presence", "installation_id": INSTALLATION, "presence": "signed_out",
        "previous_metadata_revision": 3, "metadata_revision": 4, "slot_revision": 1,
        "registry_revision": 9
    }))
    .unwrap();
    assert!(
        matches!(result, MeshResult::Presence(result) if result.presence == RegistryPresence::SignedOut)
    );
    let mut value = serde_json::to_value(row(PEER, 0, None)).unwrap();
    value["presence"] = json!("signed_out");
    let decoded: MeshManagementRow = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.presence, RegistryPresence::SignedOut);
    value["presence"] = json!("offline");
    assert!(serde_json::from_value::<MeshManagementRow>(value.clone()).is_err());
    value.as_object_mut().unwrap().remove("presence");
    assert!(serde_json::from_value::<MeshManagementRow>(value).is_err());
}

async fn enrolled() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
}

#[tokio::test]
async fn account_mesh_local_sign_out_blocks_offline_and_after_reconnect_and_restart() {
    let mut harness = enrolled().await;
    harness.log.lock().unwrap().clear();
    harness.registry.script.lock().unwrap().presence = Some(RegistryRefusal::Offline);
    harness.mesh.sign_out_local().await.unwrap();
    *harness.sessions.0.lock().unwrap() = None;
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
    assert_eq!(harness.calls("presence"), 0);
    assert_eq!(harness.registry.slot(UID), Some(1));
    harness.registry.script.lock().unwrap().presence = None;
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(AccountMeshRefusal::SignedOut)
    );
    harness.restart().await;
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(AccountMeshRefusal::SignedOut)
    );
    assert_eq!(harness.calls("presence"), 0);
    assert_eq!(harness.registry.slot(UID), Some(1));
}

#[tokio::test]
async fn account_mesh_outgoing_revocation_never_restores_the_local_session() {
    let harness = enrolled().await;
    let captured = harness.sessions.0.lock().unwrap().clone().unwrap();
    harness.mesh.sign_out_local().await.unwrap();
    *harness.sessions.0.lock().unwrap() = None;
    let result = harness
        .mesh
        .revoke_outgoing_session(&captured)
        .await
        .unwrap();
    assert_eq!(result.target_installation_id, INSTALLATION);
    assert_eq!(harness.calls("session_revoke"), 1);
    assert!(harness.sessions.0.lock().unwrap().is_none());
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
    assert_eq!(harness.registry.slot(UID), Some(1));
}

#[tokio::test]
async fn account_mesh_presence_sign_out_is_blocked_and_announced_before_credentials_release() {
    let harness = enrolled().await;
    harness.log.lock().unwrap().clear();
    let (_, persisted, ()) = harness
        .mesh
        .sign_out(async {
            assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
            assert!(harness.store.record.lock().unwrap().signed_out);
            assert_eq!(harness.registry.presence(UID), RegistryPresence::SignedOut);
            assert!(harness.sessions.0.lock().unwrap().is_some());
            let snapshots = harness.transport.snapshots.lock().unwrap();
            let grant = snapshots.last().unwrap();
            let verified = crate::account_mesh::registry_snapshot::verify_registry(
                grant,
                &Issuer::new().trust(),
                &account_id(UID),
                START,
            )
            .unwrap();
            assert!(verified
                .rows
                .iter()
                .any(|row| row.installation_id == INSTALLATION
                    && row.presence == RegistryPresence::SignedOut));
            *harness.sessions.0.lock().unwrap() = None;
        })
        .await;
    assert!(persisted.is_ok());
    assert!(
        position(&harness.log, "transport:signed_out")
            < position(&harness.log, "registry:presence:token-user-1")
    );
    assert!(
        position(&harness.log, "save:signed_out=true")
            < position(&harness.log, "registry:presence:token-user-1")
    );
    assert_eq!(harness.calls("presence"), 1);
    assert_eq!(harness.registry.slot(UID), Some(1));
}

#[tokio::test]
async fn account_mesh_presence_offline_sign_out_keeps_last_confirmed_state_and_local_block() {
    let harness = enrolled().await;
    harness.registry.script.lock().unwrap().presence = Some(RegistryRefusal::Offline);
    harness.logout().await;
    assert_eq!(harness.registry.presence(UID), RegistryPresence::Present);
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
    assert!(harness.sessions.0.lock().unwrap().is_none());
    assert_eq!(harness.calls("presence"), 1);
}

#[tokio::test]
async fn account_mesh_presence_sign_in_restores_the_retained_slot() {
    let harness = enrolled().await;
    harness.logout().await;
    assert_eq!(harness.registry.presence(UID), RegistryPresence::SignedOut);
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.registry.presence(UID), RegistryPresence::Present);
    assert_eq!(harness.state().traffic(), Traffic::Open);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(
        harness.calls("presence"),
        2,
        "sign-out announces signed_out; the renew is followed by one presence restore"
    );
}

#[tokio::test]
async fn account_mesh_presence_endpoint_withdrawal_does_not_sign_out() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    *harness.endpoint.0.lock().unwrap() = None;
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert!(harness
        .transport
        .endpoints
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .0
        .is_none());
    assert_eq!(harness.calls("withdraw"), 0);
    assert_eq!(harness.registry.presence(UID), RegistryPresence::Present);
}

#[tokio::test]
async fn account_mesh_signed_presence_gossip_rejects_unsigned_hints_without_reads() {
    let harness = enrolled().await;
    assert_eq!(
        harness.mesh.presence_notice("peer", "unsigned", 1).await,
        Ok(false)
    );
    let signed_out = registry_successor(&harness, |rows| {
        let peer = rows
            .iter_mut()
            .find(|row| row.installation_id == PEER)
            .unwrap();
        peer.presence = RegistryPresence::SignedOut;
        peer.metadata_revision = 1;
    });
    assert!(harness
        .mesh
        .receive_registry_snapshot(&signed_out)
        .await
        .unwrap());
    assert!(harness.state().unavailable_installations.contains(PEER));
    let present = registry_successor(&harness, |rows| {
        let peer = rows
            .iter_mut()
            .find(|row| row.installation_id == PEER)
            .unwrap();
        peer.presence = RegistryPresence::Present;
        peer.metadata_revision = 2;
    });
    assert!(harness
        .mesh
        .receive_registry_snapshot(&present)
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&present)
        .await
        .unwrap());
    assert!(!harness.state().unavailable_installations.contains(PEER));
    assert_eq!(harness.calls("list"), 0);
    harness.logout().await;
    assert_eq!(
        harness.mesh.receive_registry_snapshot(&present).await,
        Err(AccountMeshRefusal::SignedOut)
    );
}

#[tokio::test]
async fn account_mesh_signed_registry_gossip_refreshes_peer_membership_expiry() {
    let harness = enrolled().await;
    for expiry in [START, START + 7200] {
        let grant = registry_successor(&harness, |rows| {
            rows.iter_mut()
                .find(|row| row.installation_id == PEER)
                .unwrap()
                .membership_expires_at = Some(expiry);
        });
        assert!(harness
            .mesh
            .receive_registry_snapshot(&grant)
            .await
            .unwrap());
        assert!(!harness
            .mesh
            .receive_registry_snapshot(&grant)
            .await
            .unwrap());
        let state = harness.state();
        let peer = state
            .registry
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.installation_id == PEER)
            .unwrap();
        assert_eq!(peer.membership_expires_at, Some(expiry));
        assert_eq!(peer.presence, RegistryPresence::Present);
    }
    assert_eq!(harness.calls("list"), 0);
}
