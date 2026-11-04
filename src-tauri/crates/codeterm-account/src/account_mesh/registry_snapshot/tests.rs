use super::*;
use crate::account_mesh::tests::support::{
    count, row, Harness, Issuer, EPOCH, PEER, PROJECT, START,
};
use crate::account_mesh::{account_id, Identity};
use serde_json::json;

fn payload(harness: &Harness, revision: u64, revoked: bool) -> serde_json::Value {
    let mut rows = harness.state().registry.as_ref().unwrap().rows.clone();
    let peer = rows
        .iter_mut()
        .find(|row| row.installation_id == PEER)
        .unwrap();
    peer.active = !revoked;
    peer.metadata_revision += revision;
    if revoked {
        peer.eligibility = crate::account_mesh::wire::Eligibility::Revoked;
    }
    json!({
        "schema_version": 1, "iss": "domios-mesh-registry",
        "aud": {"project": PROJECT, "env": "test"}, "policy_epoch": EPOCH,
        "account_id": account_id("alice"), "revision": revision, "issued_at": START,
        "members": rows, "revoked_installations": if revoked { vec![PEER] } else { vec![] },
    })
}

async fn replica(installation: &str) -> Harness {
    use crate::account_mesh::{
        AccountMeshConfig, AccountMeshPorts, MembershipExpectation, SignedMembership,
    };
    let mut harness = Harness::new().await;
    harness.mesh = AccountMesh::new(
        AccountMeshConfig {
            installation_id: crate::InstallationId::parse(installation.into()).unwrap(),
            default_display_name: None,
            trust: Some(Issuer::new().trust()),
            call_deadline: std::time::Duration::from_secs(1),
            withdrawal_deadline: std::time::Duration::from_secs(1),
            schedule_leases: false,
        },
        AccountMeshPorts {
            registry: harness.registry.clone(),
            sessions: harness.sessions.clone(),
            keys: harness.keys.clone(),
            store: harness.store.clone(),
            endpoint: harness.endpoint.clone(),
            transport: harness.transport.clone(),
            clock: harness.clock.clone(),
        },
    );
    harness.mesh.load().await;
    harness.sign_in("alice").await;
    let mut membership = Issuer::membership_payload("alice", &"c".repeat(64), 1, 1, START);
    membership["installation_id"] = json!(installation);
    let issuer = Issuer::new();
    let membership = issuer.sign(&membership);
    let verified = crate::account_mesh::verify_membership(
        &membership,
        &issuer.trust(),
        &MembershipExpectation {
            account_id: &account_id("alice"),
            ..Default::default()
        },
        START,
    )
    .unwrap();
    let registry = RegistryObservation {
        observed_at: START,
        registry_revision: 1,
        rows: ["installation-a", "installation-c", "installation-d", PEER]
            .iter()
            .map(|id| row(id, 1, None))
            .collect(),
        cursor: None,
    };
    harness.mesh.with(|inner| {
        inner.update(&harness.mesh.watch, |state| {
            state.membership = Some(SignedMembership {
                token: crate::account_mesh::membership_token(&membership),
                verified,
            });
            state.registry = Some(registry);
        })
    });
    harness
}

#[test]
fn fixed_server_vector_verifies_exact_canonical_bytes() {
    let vector: serde_json::Value = serde_json::from_str(include_str!("vector.json")).unwrap();
    let grant: DetachedGrant = serde_json::from_value(vector["grant"].clone()).unwrap();
    let issuer = Issuer::new();
    let verified = verify_registry(&grant, &issuer.trust(), &account_id("alice"), START).unwrap();
    assert_eq!(verified.revision, 42);
    assert_eq!(verified.revoked_installations, [PEER.to_owned()].into());
    let value: serde_json::Value =
        serde_json::from_str(vector["canonical_payload"].as_str().unwrap()).unwrap();
    assert_eq!(issuer.sign(&value), grant);
}

#[tokio::test]
async fn rotated_cached_registry_does_not_reject_a_current_snapshot() {
    for rotation in ["key", "epoch"] {
        let mut harness = Harness::new().await;
        harness.sign_in("alice").await;
        harness
            .mesh
            .refresh(crate::account_mesh::EnrollTrigger::Automatic)
            .await
            .unwrap();
        let issuer = Issuer::new();
        let incoming = issuer.sign(&payload(&harness, 61, true));
        let mut old_payload = payload(&harness, 60, false);
        old_payload["policy_epoch"] = json!(if rotation == "epoch" {
            "retired"
        } else {
            EPOCH
        });
        let mut cached = issuer.sign(&old_payload);
        if rotation == "key" {
            cached.key_id = "retired-key".into();
        }
        harness
            .store
            .record
            .lock()
            .unwrap()
            .signed_registries
            .insert(account_id("alice"), cached);
        harness.restart().await;
        harness.sign_in("alice").await;
        assert!(harness.state().signed_registry.is_none());
        assert!(
            harness
                .mesh
                .receive_registry_snapshot(&incoming)
                .await
                .unwrap(),
            "{rotation}"
        );
        assert_eq!(
            harness.state().signed_registry.as_ref().unwrap().revision,
            61
        );
        assert!(harness.state().revoked_installations.contains(PEER));
        assert_eq!(
            harness.store.record.lock().unwrap().signed_registries[&account_id("alice")],
            incoming
        );
    }
}

#[tokio::test]
async fn launch_renewal_converges_a_day_lease_without_a_cached_snapshot() {
    let mut harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
    harness.sign_in("alice").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert!(harness.state().signed_registry.is_some());
    assert_eq!(
        harness
            .state()
            .current_membership(START)
            .unwrap()
            .verified
            .expires_at,
        START + 86400
    );
    let mut snapshot = payload(&harness, 9, true);
    for member in snapshot["members"].as_array_mut().unwrap() {
        if member["installation_id"] == super::super::tests::support::INSTALLATION {
            member["slot_revision"] = json!(2);
        }
    }
    let grant = Issuer::new().sign(&snapshot);
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.membership_registry_revision = Some(9);
        script.membership_signed_registry = Some(grant.clone());
    }
    harness
        .store
        .record
        .lock()
        .unwrap()
        .signed_registries
        .clear();
    harness.restart().await;
    harness.sign_in("alice").await;
    assert!(harness
        .store
        .record
        .lock()
        .unwrap()
        .admission_cache
        .is_some());
    harness.log.lock().unwrap().clear();
    for trigger in [
        crate::account_mesh::EnrollTrigger::Automatic,
        crate::account_mesh::EnrollTrigger::Network,
    ] {
        harness.mesh.refresh(trigger).await.unwrap();
    }
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("enroll"), 0);
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(
        harness
            .state()
            .current_membership(START)
            .unwrap()
            .verified
            .slot_revision,
        2
    );
    assert_eq!(
        harness
            .registry
            .script
            .lock()
            .unwrap()
            .renew_bodies
            .last()
            .unwrap()
            .signed_registry,
        Some(true)
    );
    assert_eq!(
        harness.state().signed_registry.as_ref().unwrap().grant,
        grant
    );
    assert!(harness.state().revoked_installations.contains(PEER));
    assert_eq!(
        harness.store.record.lock().unwrap().signed_registries[&account_id("alice")],
        grant
    );
}

#[tokio::test]
async fn signed_registry_rejects_forgery_rollback_duplicates_and_wrong_scope_atomically() {
    let harness = Harness::new().await;
    harness.sign_in("alice").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.transport.snapshots.lock().unwrap().clear();
    let issuer = Issuer::new();
    let grant = issuer.sign(&payload(&harness, 40, true));
    assert!(harness
        .mesh
        .receive_registry_snapshot(&grant)
        .await
        .unwrap());
    let before = harness.state();
    let calls = count(&harness.log, "registry:");
    assert!(before.revoked_installations.contains(PEER));
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&grant)
        .await
        .unwrap());
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&issuer.sign(&payload(&harness, 39, false)))
        .await
        .unwrap());
    let mut forged = grant.clone();
    forged.signature.replace_range(
        ..1,
        if forged.signature.starts_with('A') {
            "B"
        } else {
            "A"
        },
    );
    assert!(harness
        .mesh
        .receive_registry_snapshot(&forged)
        .await
        .is_err());
    for (field, value) in [
        ("account_id", json!(account_id("mallory"))),
        ("policy_epoch", json!("foreign")),
        ("issued_at", json!(START + NOT_BEFORE_SKEW_SECONDS + 1)),
        ("revision", json!(0)),
    ] {
        let mut invalid = payload(&harness, 41, false);
        invalid[field] = value;
        assert!(harness
            .mesh
            .receive_registry_snapshot(&issuer.sign(&invalid))
            .await
            .is_err());
    }
    assert_eq!(harness.state(), before);
    assert_eq!(count(&harness.log, "registry:"), calls);
    assert_eq!(harness.transport.snapshots.lock().unwrap().len(), 1);
    assert_eq!(
        harness
            .transport
            .last
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .revoked_installations,
        before.revoked_installations
    );
}

#[tokio::test]
async fn three_peer_revocation_gossip_and_offline_catchup_need_no_registry_calls() {
    let peers = [
        replica("installation-a").await,
        replica("installation-c").await,
        replica("installation-d").await,
    ];
    let calls: Vec<_> = peers
        .iter()
        .map(|peer| count(&peer.log, "registry:"))
        .collect();
    let grant = Issuer::new().sign(&payload(&peers[0], 50, true));
    assert!(peers[0]
        .mesh
        .receive_registry_snapshot(&grant)
        .await
        .unwrap());
    let delivered = peers[0].transport.snapshots.lock().unwrap().pop().unwrap();
    assert!(peers[1]
        .mesh
        .receive_registry_snapshot(&delivered)
        .await
        .unwrap());
    assert!(!peers[0]
        .mesh
        .receive_registry_snapshot(&delivered)
        .await
        .unwrap());
    assert!(!peers[2].state().revoked_installations.contains(PEER));
    let catchup = peers[1]
        .state()
        .signed_registry
        .as_ref()
        .unwrap()
        .grant
        .clone();
    assert!(peers[2]
        .mesh
        .receive_registry_snapshot(&catchup)
        .await
        .unwrap());
    for (index, peer) in peers.iter().enumerate() {
        assert_eq!(count(&peer.log, "registry:"), calls[index]);
        assert!(peer.state().revoked_installations.contains(PEER));
        assert_eq!(peer.state().signed_registry.as_ref().unwrap().revision, 50);
    }
}

#[tokio::test]
async fn persisted_revision_rejects_rollback_after_controller_restart() {
    let mut harness = Harness::new().await;
    harness.sign_in("alice").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    let issuer = Issuer::new();
    let previous = issuer.sign(&payload(&harness, 59, false));
    let latest = issuer.sign(&payload(&harness, 60, true));
    harness
        .mesh
        .receive_registry_snapshot(&latest)
        .await
        .unwrap();
    harness.restart().await;
    harness
        .mesh
        .identity_changed(Some(&Identity {
            uid: "alice".into(),
            email: None,
            display_name: None,
            provider: "test".into(),
            photo_url: None,
        }))
        .await;
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&previous)
        .await
        .unwrap());
    assert_eq!(
        harness.state().signed_registry.as_ref().unwrap().revision,
        60
    );
    assert!(harness.state().revoked_installations.contains(PEER));
}

#[tokio::test]
async fn server_signout_snapshot_is_gossiped_after_the_local_block() {
    let harness = Harness::new().await;
    harness.sign_in("alice").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.transport.snapshots.lock().unwrap().clear();
    let mut signed_out = payload(&harness, 70, false);
    for row in signed_out["members"].as_array_mut().unwrap() {
        if row["installation_id"] == super::super::tests::support::INSTALLATION {
            row["presence"] = json!("signed_out");
        }
    }
    let grant = Issuer::new().sign(&signed_out);
    harness.mesh.sign_out_local().await.unwrap();
    harness
        .mesh
        .accept_server_registry(&grant, &account_id("alice"), 0)
        .await
        .unwrap();
    assert_eq!(*harness.transport.snapshots.lock().unwrap(), vec![grant]);
    assert!(harness.state().signed_in_account().is_none());
    assert!(harness.state().current_membership(START).is_none());
}

#[tokio::test]
async fn signedout_presence_blocks_immediately_and_newer_present_revision_restores_peer() {
    let harness = Harness::new().await;
    harness.sign_in("alice").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    let mut signed_out = payload(&harness, 80, false);
    for row in signed_out["members"].as_array_mut().unwrap() {
        if row["installation_id"] == PEER {
            row["presence"] = json!("signed_out");
        }
    }
    let grant = Issuer::new().sign(&signed_out);
    harness
        .mesh
        .receive_registry_snapshot(&grant)
        .await
        .unwrap();
    assert!(harness.state().unavailable_installations.contains(PEER));
    assert!(!harness.state().revoked_installations.contains(PEER));
    signed_out["revision"] = json!(81);
    for row in signed_out["members"].as_array_mut().unwrap() {
        if row["installation_id"] == PEER {
            row["presence"] = json!("present");
        }
    }
    harness
        .mesh
        .receive_registry_snapshot(&Issuer::new().sign(&signed_out))
        .await
        .unwrap();
    assert!(harness.state().unavailable_installations.contains(PEER));
    signed_out["revision"] = json!(82);
    for row in signed_out["members"].as_array_mut().unwrap() {
        if row["installation_id"] == PEER {
            row["metadata_revision"] = json!(81);
        }
    }
    assert!(harness
        .mesh
        .receive_registry_snapshot(&Issuer::new().sign(&signed_out))
        .await
        .unwrap());
    assert!(!harness.state().unavailable_installations.contains(PEER));
}
