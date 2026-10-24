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
            assert!(
                position(&harness.log, "registry:presence:token-user-1")
                    < position(&harness.log, "transport:presence:")
            );
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
    assert_eq!(harness.calls("presence"), 2);
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
}

#[tokio::test]
async fn account_mesh_presence_endpoint_withdrawal_does_not_sign_out() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    *harness.endpoint.0.lock().unwrap() = None;
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert!(harness.registry.endpoint(UID).is_none());
    assert_eq!(harness.registry.presence(UID), RegistryPresence::Present);
}

#[tokio::test]
async fn account_mesh_presence_notice_triggers_one_registry_read_without_snapshot_feedback() {
    let harness = enrolled().await;
    let before = harness.calls("list");
    harness.registry.script.lock().unwrap().peer_presence = RegistryPresence::SignedOut;
    assert_eq!(harness.mesh.presence_notice("peer", 1).await, Ok(true));
    assert_eq!(harness.calls("list"), before + 1);
    assert_eq!(harness.mesh.presence_notice("peer", 1).await, Ok(false));
    assert_eq!(harness.calls("list"), before + 1);
    let peer = harness
        .state()
        .registry
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.installation_id == PEER)
        .unwrap()
        .clone();
    assert_eq!(peer.presence, RegistryPresence::SignedOut);
    harness.registry.script.lock().unwrap().peer_presence = RegistryPresence::Present;
    assert_eq!(harness.mesh.presence_notice("peer", 2).await, Ok(true));
    assert_eq!(harness.calls("list"), before + 2);
    let peer = harness
        .state()
        .registry
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.installation_id == PEER)
        .unwrap()
        .clone();
    assert_eq!(peer.presence, RegistryPresence::Present);
    harness.logout().await;
    let blocked = harness.calls("list");
    assert_eq!(
        harness.mesh.presence_notice("peer", 2).await,
        Err(AccountMeshRefusal::SignedOut)
    );
    assert_eq!(harness.calls("list"), blocked);
}
