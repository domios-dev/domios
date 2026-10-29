use super::support::*;
use crate::account_mesh::*;
use wire::RegistryPresence;

fn peer_membership(key: &InstallationKey, generation: u64) -> String {
    let mut payload = Issuer::membership_payload("owner", &key.key_id(), generation, 1, START);
    payload["installation_id"] = serde_json::json!(PEER);
    membership_token(&Issuer::new().sign(&payload))
}

async fn enrolled() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
}

fn assert_blocked(harness: &Harness, token: &str, blocked: bool) {
    assert_eq!(
        harness.mesh.verify_peer_membership(token, START).is_err(),
        blocked
    );
    assert_eq!(
        harness.state().revoked_installations.contains(PEER),
        blocked
    );
    assert_eq!(
        harness
            .transport
            .last
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .revoked_installations
            .contains(PEER),
        blocked
    );
}

#[tokio::test]
async fn account_mesh_registry_revocation_blocks_on_list_and_requires_higher_generation() {
    let mut harness = enrolled().await;
    let key = InstallationKey::generate();
    harness.registry.script.lock().unwrap().peer_key = Some((key.key_id(), 1));
    harness.mesh.list().await.unwrap();
    let token = peer_membership(&key, 1);
    assert_blocked(&harness, &token, false);
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness.registry.script.lock().unwrap().peer_active = Some(true);
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness.restart().await;
    harness.sign_in("owner").await;
    assert_blocked(&harness, &token, true);
    let next_key = InstallationKey::generate();
    harness.registry.script.lock().unwrap().peer_key = Some((next_key.key_id(), 2));
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &peer_membership(&next_key, 2), false);
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    harness.registry.script.lock().unwrap().peer_key = Some((key.key_id(), 1));
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &peer_membership(&next_key, 2), false);
}

#[tokio::test]
async fn account_mesh_registry_signed_out_blocks_on_sync_and_fences_stale_presence() {
    let mut harness = enrolled().await;
    let key = InstallationKey::generate();
    harness.registry.script.lock().unwrap().peer_key = Some((key.key_id(), 1));
    harness.mesh.list().await.unwrap();
    let token = peer_membership(&key, 1);
    assert_blocked(&harness, &token, false);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::SignedOut;
        script.peer_metadata_revision = 2;
    }
    let signed = harness.mesh.signed(None).await.unwrap();
    harness.mesh.sync_registry(&signed).await.unwrap();
    assert_blocked(&harness, &token, true);
    harness.restart().await;
    harness.sign_in("owner").await;
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::Present;
        script.peer_metadata_revision = 1;
    }
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .peer_metadata_revision = 2;
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .peer_metadata_revision = 3;
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, false);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::SignedOut;
        script.peer_metadata_revision = 2;
    }
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, false);
}

#[tokio::test]
async fn account_mesh_registry_presence_notice_readmits_without_waiting_for_lease() {
    let harness = enrolled().await;
    let key = InstallationKey::generate();
    let token = peer_membership(&key, 1);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_key = Some((key.key_id(), 1));
        script.peer_presence = RegistryPresence::SignedOut;
        script.peer_metadata_revision = 2;
    }
    harness.mesh.list().await.unwrap();
    assert_blocked(&harness, &token, true);
    let membership = harness.state().membership.clone().unwrap();
    let before = harness.calls("list");
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::Present;
        script.peer_metadata_revision = 3;
    }
    assert!(harness
        .mesh
        .presence_notice("peer", "fresh-sign-in", 3)
        .await
        .unwrap());
    assert_eq!(harness.calls("list"), before + 1);
    assert_eq!(harness.clock.now(), START);
    assert_eq!(harness.state().membership.clone().unwrap(), membership);
    assert_blocked(&harness, &token, false);
}

#[tokio::test]
async fn account_mesh_heartbeat_higher_revision_lists_once_equal_revision_is_free() {
    let harness = enrolled().await;
    let account = account_id("owner");
    let before = harness.calls("list");
    harness.registry.script.lock().unwrap().registry_revision = Some(10);
    assert!(harness
        .mesh
        .registry_hint(
            "peer",
            "heartbeat-1",
            &account,
            Some(10),
            Some("changed-security")
        )
        .await
        .unwrap());
    assert_eq!(harness.calls("list"), before + 1);
    assert_eq!(
        harness.state().registry.as_ref().unwrap().registry_revision,
        10
    );
    let calls = count(&harness.log, "registry:");
    assert!(!harness
        .mesh
        .registry_hint(
            "peer",
            "heartbeat-2",
            &account,
            Some(10),
            Some("changed-security")
        )
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .registry_hint(
            "peer",
            "heartbeat-3",
            &account,
            Some(9),
            Some("changed-security")
        )
        .await
        .unwrap());
    assert_eq!(count(&harness.log, "registry:"), calls);
    assert_eq!(
        harness.state().registry.as_ref().unwrap().registry_revision,
        10
    );
    assert_eq!(harness.calls("sync"), 1);
    assert_eq!(
        harness
            .mesh
            .registry_hint(
                "foreign",
                "foreign",
                "other-account",
                Some(11),
                Some("foreign-security")
            )
            .await,
        Err(AccountMeshRefusal::Forbidden)
    );
    assert_eq!(count(&harness.log, "registry:"), calls);
}

#[tokio::test]
async fn account_mesh_concurrent_heartbeat_hints_share_one_list() {
    let harness = enrolled().await;
    let account = account_id("owner");
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.registry_revision = Some(10);
        script.list_gate = Some(gate.clone());
    }
    let before = harness.calls("list");
    let first = harness.mesh.registry_hint(
        "peer-a",
        "first",
        &account,
        Some(10),
        Some("changed-security"),
    );
    let second = async {
        harness.registry.list_entered.notified().await;
        let result = harness
            .mesh
            .registry_hint(
                "peer-b",
                "second",
                &account,
                Some(10),
                Some("changed-security"),
            )
            .await;
        gate.notify_one();
        result
    };
    let (first, second) = tokio::join!(biased; first, second);
    assert_eq!(first, Ok(true));
    assert_eq!(second, Ok(false));
    assert_eq!(harness.calls("list"), before + 1);
}

#[tokio::test]
async fn account_mesh_remote_revoke_notifies_and_peer_observation_blocks() {
    for signed_out in [false, true] {
        let actor = enrolled().await;
        if signed_out {
            actor.mesh.revoke_session(PEER.into()).await.unwrap();
        } else {
            actor
                .mesh
                .revoke(row(PEER, 0, None).target())
                .await
                .unwrap();
        }
        assert_eq!(
            *actor.transport.revoked.lock().unwrap(),
            vec![PEER.to_owned()]
        );
        let receiver = enrolled().await;
        let key = InstallationKey::generate();
        let token = peer_membership(&key, 1);
        {
            let mut script = receiver.registry.script.lock().unwrap();
            script.peer_key = Some((key.key_id(), 1));
        }
        receiver.mesh.list().await.unwrap();
        assert_blocked(&receiver, &token, false);
        {
            let mut script = receiver.registry.script.lock().unwrap();
            script.peer_active = Some(signed_out);
            script.peer_presence = if signed_out {
                RegistryPresence::SignedOut
            } else {
                RegistryPresence::Present
            };
            script.peer_metadata_revision = 1;
            script.registry_revision = Some(10);
        }
        assert!(receiver
            .mesh
            .registry_hint(
                "actor",
                "signed-revocation-notice",
                &account_id("owner"),
                None,
                None,
            )
            .await
            .unwrap());
        assert_blocked(&receiver, &token, true);
    }
}

#[tokio::test]
async fn account_mesh_long_lease_restore_observes_revocation_and_keeps_cached_membership_offline() {
    for offline in [false, true] {
        let mut harness = Harness::new().await;
        harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
        harness.sign_in("owner").await;
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        harness.restart().await;
        harness.sign_in("owner").await;
        harness.log.lock().unwrap().clear();
        {
            let mut script = harness.registry.script.lock().unwrap();
            script.peer_active = Some(false);
            if offline {
                script.list = Some(wire::RegistryRefusal::Offline);
            }
        }
        let result = harness.mesh.refresh(EnrollTrigger::Automatic).await;
        assert_eq!(harness.calls("list"), 1);
        if offline {
            assert_eq!(result, Err(AccountMeshRefusal::Offline));
            assert!(harness
                .transport
                .last
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .membership
                .as_ref()
                .is_some_and(|membership| membership.verified.is_current(START)));
        } else {
            result.unwrap();
            assert!(harness
                .transport
                .last
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .revoked_installations
                .contains(PEER));
        }
        assert_eq!(harness.calls("renew"), 0);
    }
}

#[tokio::test]
async fn account_mesh_heartbeat_lease_revision_churn_has_zero_registry_calls() {
    let harness = enrolled().await;
    let state = harness.state();
    let security = state.registry_security.as_ref().unwrap();
    let calls = count(&harness.log, "registry:");
    for revision in [security.revision + 1, security.revision + 100] {
        assert!(!harness
            .mesh
            .registry_hint(
                "peer",
                "lease-heartbeat",
                &account_id("owner"),
                Some(revision),
                Some(&security.digest)
            )
            .await
            .unwrap());
    }
    assert_eq!(count(&harness.log, "registry:"), calls);
}

#[test]
fn account_mesh_security_digest_ignores_lease_endpoint_and_metadata_churn() {
    let mut observation = RegistryObservation {
        observed_at: START,
        registry_revision: 1,
        rows: vec![row("a", 1, None), row("b", 1, None)],
        cursor: None,
    };
    let original = observation.security_view().unwrap().digest;
    observation.rows.reverse();
    observation.registry_revision = 100;
    for row in &mut observation.rows {
        row.slot_revision += 10;
        row.metadata_revision += 10;
        row.display_name = Some("renamed".into());
        row.authenticated_at += 10;
        row.membership_expires_at = Some(START + 86400);
        row.publication = wire::PublicationState::None;
        row.endpoint = None;
    }
    assert_eq!(observation.security_view().unwrap().digest, original);
    observation.rows[0].active = false;
    assert_ne!(observation.security_view().unwrap().digest, original);
    observation.rows[0].active = true;
    observation.rows[0].presence = RegistryPresence::SignedOut;
    assert_ne!(observation.security_view().unwrap().digest, original);
    observation.cursor = Some(wire::MeshCursor {
        after_installation_id: "b".into(),
        registry_revision: 100,
    });
    assert!(observation.security_view().is_none());
}

#[tokio::test]
async fn account_mesh_partial_sync_preserves_last_complete_security_view() {
    let harness = enrolled().await;
    let previous = harness.state().registry_security.clone().unwrap();
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.registry_revision = Some(10);
        script.peer_presence = RegistryPresence::SignedOut;
        script.peer_metadata_revision = 1;
        script.sync_cursor = Some(wire::MeshCursor {
            after_installation_id: INSTALLATION.into(),
            registry_revision: 10,
        });
    }
    let signed = harness.mesh.signed(None).await.unwrap();
    harness.mesh.sync_registry(&signed).await.unwrap();
    assert_eq!(harness.state().registry_security.as_ref(), Some(&previous));
    harness.mesh.list().await.unwrap();
    let state = harness.state();
    let current = state.registry_security.as_ref().unwrap();
    assert_eq!(current.revision, 10);
    assert_ne!(current.digest, previous.digest);
}
