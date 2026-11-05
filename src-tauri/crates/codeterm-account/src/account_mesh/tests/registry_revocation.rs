use super::support::*;
use crate::account_mesh::*;
use wire::RegistryPresence;

fn peer_membership(key: &InstallationKey, generation: u64) -> String {
    let mut payload = Issuer::membership_payload("owner", &key.key_id(), generation, 1, START);
    payload["installation_id"] = serde_json::json!(PEER);
    membership_token(&Issuer::new().sign(&payload))
}

async fn enrolled() -> Harness {
    enrolled_with_peer(None).await
}

async fn enrolled_with_peer(key: Option<&InstallationKey>) -> Harness {
    let harness = Harness::new().await;
    if let Some(key) = key {
        harness.registry.script.lock().unwrap().peer_key = Some((key.key_id(), 1));
    }
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
}

fn assert_blocked(harness: &Harness, token: &str, blocked: bool) {
    let state = harness.state();
    let revoked = state.revoked_installations.contains(PEER);
    let unavailable = state.unavailable_installations.contains(PEER);
    assert_eq!(revoked || unavailable, blocked);
    assert_eq!(
        harness.mesh.verify_peer_membership(token, START).is_err(),
        blocked
    );
    let transport = harness.transport.last.lock().unwrap();
    let applied = transport.as_ref().unwrap();
    assert_eq!(applied.revoked_installations.contains(PEER), revoked);
    assert_eq!(
        applied.unavailable_installations.contains(PEER),
        unavailable
    );
}

#[tokio::test]
async fn account_mesh_registry_revocation_blocks_on_list_and_requires_higher_generation() {
    let key = InstallationKey::generate();
    let mut harness = enrolled_with_peer(Some(&key)).await;
    harness.mesh.list_fresh().await.unwrap();
    let token = peer_membership(&key, 1);
    assert_blocked(&harness, &token, false);
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list_fresh().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness.registry.script.lock().unwrap().peer_active = Some(true);
    harness.mesh.list_fresh().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness.restart().await;
    harness.sign_in("owner").await;
    assert_blocked(&harness, &token, true);
    let next_key = InstallationKey::generate();
    harness.registry.script.lock().unwrap().peer_key = Some((next_key.key_id(), 2));
    harness.mesh.list_fresh().await.unwrap();
    assert_blocked(&harness, &peer_membership(&next_key, 2), false);
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    harness.registry.script.lock().unwrap().peer_key = Some((key.key_id(), 1));
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    assert_eq!(
        harness.mesh.list_fresh().await,
        Err(AccountMeshRefusal::Invalid)
    );
    assert_blocked(&harness, &peer_membership(&next_key, 2), false);
}

#[tokio::test]
async fn account_mesh_registry_signed_out_blocks_on_sync_and_fences_stale_presence() {
    let key = InstallationKey::generate();
    let mut harness = enrolled_with_peer(Some(&key)).await;
    harness.mesh.list_fresh().await.unwrap();
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
    assert!(!harness.state().revoked_installations.contains(PEER));
    assert!(!harness
        .store
        .record
        .lock()
        .unwrap()
        .revoked_generations
        .contains_key(PEER));
    harness.restart().await;
    harness.sign_in("owner").await;
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::Present;
        script.peer_metadata_revision = 1;
    }
    assert_eq!(
        harness.mesh.list_fresh().await,
        Err(AccountMeshRefusal::Invalid)
    );
    assert_blocked(&harness, &token, true);
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .peer_metadata_revision = 2;
    harness.mesh.list_fresh().await.unwrap();
    assert_blocked(&harness, &token, true);
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .peer_metadata_revision = 3;
    harness.mesh.list_fresh().await.unwrap();
    assert_blocked(&harness, &token, false);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.peer_presence = RegistryPresence::SignedOut;
        script.peer_metadata_revision = 2;
    }
    let before = harness.state().signed_registry.clone();
    assert_eq!(
        harness.mesh.list_fresh().await,
        Err(AccountMeshRefusal::Invalid)
    );
    assert_eq!(harness.state().signed_registry, before);
    assert_blocked(&harness, &token, false);
}

#[tokio::test]
async fn account_mesh_signed_presence_gossip_readmits_without_waiting_for_lease() {
    let key = InstallationKey::generate();
    let harness = enrolled_with_peer(Some(&key)).await;
    let token = peer_membership(&key, 1);
    for (revision, presence, blocked) in [
        (2, RegistryPresence::SignedOut, true),
        (3, RegistryPresence::Present, false),
    ] {
        let grant = registry_successor(&harness, |rows| {
            let peer = rows
                .iter_mut()
                .find(|row| row.installation_id == PEER)
                .unwrap();
            peer.presence = presence;
            peer.metadata_revision = revision;
        });
        assert!(harness
            .mesh
            .receive_registry_snapshot(&grant)
            .await
            .unwrap());
        assert_blocked(&harness, &token, blocked);
    }
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.calls("renew"), 0);
    assert!(harness.state().current_membership(START).is_some());
}

#[tokio::test]
async fn account_mesh_remote_revoke_notifies_transport() {
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
    }
}

#[tokio::test]
async fn account_mesh_long_lease_restore_observes_revocation_and_keeps_cached_membership_offline() {
    let mut harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let revoked = registry_successor(&harness, |rows| {
        rows.iter_mut()
            .find(|row| row.installation_id == PEER)
            .unwrap()
            .active = false
    });
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.log.lock().unwrap().clear();
    harness.registry.script.lock().unwrap().membership = Some(wire::RegistryRefusal::Offline);
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert!(harness
        .mesh
        .receive_registry_snapshot(&revoked)
        .await
        .unwrap());
    for _ in 0..3 {
        harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    }
    assert!(harness.state().revoked_installations.contains(PEER));
    assert!(harness.state().current_membership(START).is_some());
    assert_eq!(count(&harness.log, "registry:"), 0);
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
async fn account_mesh_signed_complete_view_supersedes_unsigned_page_cursor() {
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
    harness.mesh.list_fresh().await.unwrap();
    let state = harness.state();
    assert!(state.registry.as_ref().unwrap().cursor.is_none());
    let current = state.registry_security.as_ref().unwrap();
    assert_eq!(current.revision, previous.revision + 1);
    assert_ne!(current.revision, 10);
    assert_eq!(
        current.revision,
        state.signed_registry.as_ref().unwrap().revision
    );
    assert_ne!(current.digest, previous.digest);
    assert!(state.unavailable_installations.contains(PEER));
}

#[tokio::test]
async fn account_mesh_old_signed_out_snapshot_cannot_clear_fresh_signin_membership() {
    let harness = enrolled().await;
    let signed_out = registry_successor(&harness, |rows| {
        let own = rows
            .iter_mut()
            .find(|row| row.installation_id == INSTALLATION)
            .unwrap();
        own.presence = RegistryPresence::SignedOut;
        own.metadata_revision += 1;
    });
    harness.logout().await;
    harness.sign_in("owner").await;
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    let fresh = harness.state().membership.clone().unwrap();
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&signed_out)
        .await
        .unwrap());
    assert_eq!(harness.state().membership.as_ref(), Some(&fresh));
    assert!(harness.state().current_membership(START).is_some());
    assert_eq!(harness.calls("recover_key"), 0);
}

#[tokio::test]
async fn account_mesh_signed_cache_rebuilds_presence_without_server_reads() {
    let mut harness = enrolled().await;
    harness.store.record.lock().unwrap().peer_presence_refresh = true;
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.log.lock().unwrap().clear();
    harness.registry.script.lock().unwrap().list = Some(wire::RegistryRefusal::Offline);
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert!(harness.state().current_membership(START).is_some());
    assert!(!harness.store.record.lock().unwrap().peer_presence_refresh);
    assert_eq!(count(&harness.log, "registry:"), 0);
}

#[tokio::test]
async fn account_mesh_corrupt_availability_list_rate_limit_does_not_skip_due_renewal() {
    let harness = enrolled().await;
    let original = harness.state().membership.clone().unwrap();
    harness.registry.script.lock().unwrap().list = Some(wire::RegistryRefusal::RateLimited {
        retry_after_seconds: 60,
    });
    assert_eq!(
        harness.mesh.list_fresh().await,
        Err(AccountMeshRefusal::RateLimited {
            retry_after_seconds: 60
        })
    );
    harness.clock.set(original.verified.refresh_after());
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("list"), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert!(
        harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .expires_at
            > original.verified.expires_at
    );
    assert_eq!(harness.calls("recover_key"), 0);
}
