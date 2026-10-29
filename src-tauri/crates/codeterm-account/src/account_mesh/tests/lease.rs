use super::support::*;
use crate::account_mesh::wire::*;
use crate::account_mesh::*;
use serde_json::json;

async fn leased(seconds: u64) -> Harness {
    let harness = Harness::new().await;
    harness.registry.script.lock().unwrap().membership_seconds = Some(seconds);
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.log.lock().unwrap().clear();
    harness
}

#[tokio::test]
async fn account_mesh_lease_accepts_hour_and_day_and_reuses_after_restart() {
    for seconds in [3600, 86400] {
        let mut harness = leased(seconds).await;
        let original = harness.state().membership.clone().unwrap();
        assert_eq!(original.verified.expires_at, START + seconds);
        assert_eq!(
            original.verified.refresh_after(),
            START + seconds - (seconds / 4).max(120)
        );
        assert_eq!(
            harness.mesh.verify_peer_membership(&original.token, START),
            Ok(original.verified.clone())
        );
        harness.restart().await;
        harness.sign_in("owner").await;
        harness.log.lock().unwrap().clear();
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        assert_eq!(count(&harness.log, "registry:"), 0);
        assert_eq!(
            harness.state().membership.as_ref().unwrap().verified,
            original.verified
        );
    }
}

#[tokio::test]
async fn account_mesh_lease_expired_startup_renews_once_without_redundant_sync() {
    let mut harness = leased(86400).await;
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .membership_registry_revision = Some(9);
    harness.clock.set(START + 86400);
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.log.lock().unwrap().clear();
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("challenge"), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("sync"), 0);
    assert!(harness.state().current_membership(START + 86400).is_some());
}

#[tokio::test]
async fn account_mesh_lease_changed_registry_syncs_after_renew() {
    let harness = leased(86400).await;
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .membership_registry_revision = Some(11);
    harness.clock.set(START + 86400 - 120);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert_eq!(harness.calls("challenge"), 2);
}

#[tokio::test]
async fn account_mesh_lease_observed_revocation_and_signout_fence_long_membership() {
    let mut harness = leased(86400).await;
    let membership = harness.state().membership.clone().unwrap();
    let target = MeshTarget {
        installation_id: membership.verified.installation_id.clone(),
        key_id: membership.verified.key_id.clone(),
        key_generation: membership.verified.key_generation,
        slot_revision: membership.verified.slot_revision,
    };
    harness.mesh.revoke(target).await.unwrap();
    assert!(harness.state().current_membership(START).is_none());
    assert_eq!(
        harness
            .mesh
            .verify_peer_membership(&membership.token, START),
        Err(MembershipRefusal::Key)
    );
    harness.restart().await;
    harness.sign_in("owner").await;
    assert!(harness.state().current_membership(START).is_none());
    harness.logout().await;
    assert!(harness
        .store
        .record
        .lock()
        .unwrap()
        .admission_cache
        .is_none());
    assert_eq!(
        harness
            .mesh
            .verify_peer_membership(&membership.token, START),
        Err(MembershipRefusal::Subject)
    );
}

#[tokio::test]
async fn account_mesh_lease_rejects_longer_signed_peer_without_revoking_it() {
    let harness = leased(86400).await;
    let own = harness.state().membership.clone().unwrap();
    let mut payload = Issuer::membership_payload("owner", &own.verified.key_id, 1, 1, START);
    payload["expires_at"] = json!(START + 86401);
    let token = membership_token(&Issuer::new().sign(&payload));
    assert_eq!(
        harness.mesh.verify_peer_membership(&token, START),
        Err(MembershipRefusal::Duration)
    );
    assert!(harness.state().revoked_installations.is_empty());
    assert!(harness
        .mesh
        .verify_peer_membership(&own.token, START)
        .is_ok());
}

#[test]
fn account_mesh_lease_opt_in_is_additive_for_an_old_request_reader() {
    #[derive(serde::Deserialize)]
    struct OldRequest {
        schema_version: u8,
        installation_id: String,
        operation: String,
    }
    let body = MeshAdmissionBody {
        schema_version: 1,
        installation_id: INSTALLATION.into(),
        request_id: "a".repeat(64),
        requested_membership_seconds: Some(86400),
        operation: MeshOperation::Renew {
            expected_slot_revision: 1,
        },
    };
    let value = serde_json::to_value(body).unwrap();
    assert_eq!(value["requested_membership_seconds"], 86400);
    let old: OldRequest = serde_json::from_value(value).unwrap();
    assert_eq!(
        (
            old.schema_version,
            old.installation_id.as_str(),
            old.operation.as_str()
        ),
        (1, INSTALLATION, "renew")
    );
}

#[tokio::test]
async fn account_mesh_lease_persisted_signout_never_restores_a_cached_membership() {
    let mut harness = leased(86400).await;
    harness.store.record.lock().unwrap().signed_out = true;
    harness.restart().await;
    harness.sign_in("owner").await;
    assert!(harness
        .store
        .record
        .lock()
        .unwrap()
        .admission_cache
        .is_none());
    assert!(harness.state().current_membership(START).is_none());
}

#[tokio::test]
async fn account_mesh_lease_own_renew_adopts_one_revision_and_current_slot() {
    let harness = leased(86400).await;
    let before = harness
        .state()
        .self_installation
        .as_ref()
        .unwrap()
        .slot_revision;
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .membership_registry_revision = Some(10);
    harness.clock.set(START + 86400 - 120);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(harness.calls("challenge"), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("sync"), 0);
    let state = harness.state();
    assert_eq!(state.registry.as_ref().unwrap().registry_revision, 10);
    assert_eq!(
        state.self_installation.as_ref().unwrap().slot_revision,
        before + 1
    );
    let own = state
        .registry
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.installation_id == INSTALLATION)
        .unwrap();
    assert_eq!(own.slot_revision, before + 1);
    harness
        .mesh
        .authorize_link(LinkAuthorization {
            transcript: [3; 32],
            local_installation_id: INSTALLATION.into(),
            peer_installation_id: PEER.into(),
            account_id: account_id("owner"),
        })
        .await
        .unwrap();
    assert_eq!(harness.calls("link"), 1);
}

#[tokio::test(start_paused = true)]
async fn account_mesh_lease_schedules_short_endpoint_before_long_membership() {
    scheduled_publication(3600, 3480, 0).await;
}

#[tokio::test(start_paused = true)]
async fn account_mesh_lease_equal_expiries_share_one_refresh() {
    scheduled_publication(86400, 64800, 1).await;
}

async fn scheduled_publication(endpoint_seconds: u64, due: u64, renewals: usize) {
    let harness = Harness::with_leases().await;
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.membership_seconds = Some(86400);
        script.endpoint_seconds = Some(endpoint_seconds);
        script.membership_registry_revision = Some(10);
    }
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let revision = harness
        .state()
        .self_installation
        .as_ref()
        .unwrap()
        .metadata_revision;
    harness.log.lock().unwrap().clear();
    let mut state = harness.mesh.subscribe();
    harness.clock.set(START + due);
    let started = tokio::time::Instant::now();
    tokio::time::timeout(
        std::time::Duration::from_secs(due + 1),
        state.wait_for(|state| {
            state.enrollment == Enrollment::Enrolled
                && state
                    .self_installation
                    .as_ref()
                    .is_some_and(|own| own.metadata_revision > revision)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(started.elapsed() <= std::time::Duration::from_secs(60));
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("renew"), renewals);
    assert_eq!(harness.calls("publish"), 1);
    assert_eq!(harness.calls("sync"), 0);
    assert_eq!(harness.calls("challenge"), renewals + 1);
    assert!(harness.registry.endpoint("owner").unwrap().expires_at > START + due);
    harness.mesh.sign_out_local().await.unwrap();
}

#[tokio::test]
async fn account_mesh_lease_restore_rejects_changed_authority_and_tampered_signature() {
    for case in ["valid", "account", "key", "generation", "slot", "signature"] {
        let mut harness = leased(86400).await;
        let original = harness.state().membership.clone().unwrap();
        let account = account_id("owner");
        match case {
            "key" => {
                *harness.keys.0.lock().unwrap() = Some(InstallationKey::generate().to_secret());
            }
            "generation" => {
                harness
                    .store
                    .record
                    .lock()
                    .unwrap()
                    .keys
                    .get_mut(&account)
                    .unwrap()
                    .generation += 1;
            }
            "slot" => {
                *harness
                    .store
                    .record
                    .lock()
                    .unwrap()
                    .slots
                    .get_mut(&account)
                    .unwrap() += 1;
            }
            "signature" => {
                let mut grant = parse_membership_token(&original.token).unwrap();
                grant.signature = "a".repeat(86);
                let mut record = harness.store.record.lock().unwrap();
                let mut value = serde_json::to_value(&*record).unwrap();
                value["admission_cache"]["token"] = json!(membership_token(&grant));
                *record = serde_json::from_value(value).unwrap();
            }
            _ => {}
        }
        harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Offline);
        harness.restart().await;
        harness
            .sign_in(if case == "account" { "other" } else { "owner" })
            .await;
        harness.log.lock().unwrap().clear();
        let result = harness.mesh.refresh(EnrollTrigger::Automatic).await;
        if case == "valid" {
            assert_eq!(result, Ok(()));
            assert_eq!(count(&harness.log, "registry:"), 0);
            assert_eq!(
                harness.state().membership.as_ref().unwrap().verified,
                original.verified
            );
        } else {
            assert!(
                result.is_err(),
                "{case}: invalid cache must require online admission"
            );
            assert!(
                harness.calls("challenge") > 0,
                "{case}: authenticated handshake required"
            );
            assert!(
                harness.state().membership.is_none(),
                "{case}: cached token was admitted"
            );
        }
    }
}
