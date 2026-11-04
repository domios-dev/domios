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
        assert!(original.verified.refresh_after() > START);
        assert!(original.verified.refresh_after() < START + seconds);
        assert_eq!(
            harness.mesh.verify_peer_membership(&original.token, START),
            Ok(original.verified.clone())
        );
        harness.restart().await;
        harness.sign_in("owner").await;
        harness
            .registry
            .script
            .lock()
            .unwrap()
            .membership_registry_revision = Some(9);
        harness.log.lock().unwrap().clear();
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        for trigger in [
            EnrollTrigger::Automatic,
            EnrollTrigger::Network,
            EnrollTrigger::Lease,
        ] {
            harness.mesh.refresh(trigger).await.unwrap();
        }
        assert_eq!(harness.calls("list"), 0);
        assert_eq!(harness.calls("renew"), 0);
        assert_eq!(harness.calls("enroll_key"), 0);
        assert_eq!(harness.calls("link"), 0);
        assert!(harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .is_current(START));
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
async fn account_mesh_launch_reuses_membership_when_issuer_clock_is_ahead() {
    let mut harness = Harness::new().await;
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.issuer_ahead = 120;
        script.membership_seconds = Some(86400);
        script.membership_registry_revision = Some(9);
    }
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(
        harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .issued_at,
        START + 120
    );
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.log.lock().unwrap().clear();
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(harness.calls("link"), 0);
}

#[tokio::test]
async fn account_mesh_lease_changed_registry_is_bundled_with_renew() {
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
    assert_eq!(harness.calls("sync"), 0);
    assert_eq!(harness.calls("challenge"), 1);
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
async fn account_mesh_lease_accepts_server_controlled_lifetime() {
    let harness = leased(86400).await;
    let own = harness.state().membership.clone().unwrap();
    let mut payload = Issuer::membership_payload("owner", &own.verified.key_id, 1, 1, START);
    payload["expires_at"] = json!(START + 86401);
    let token = membership_token(&Issuer::new().sign(&payload));
    assert_eq!(
        harness.mesh.verify_peer_membership(&token, START),
        Ok(VerifiedMembership {
            expires_at: START + 86401,
            ..own.verified.clone()
        })
    );
    assert!(harness.state().revoked_installations.is_empty());
    assert!(harness
        .mesh
        .verify_peer_membership(&own.token, START)
        .is_ok());
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
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.calls("link"), 0);
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
        .registry
        .script
        .lock()
        .unwrap()
        .membership_registry_revision = Some(10);
    let due = harness
        .state()
        .membership
        .as_ref()
        .unwrap()
        .verified
        .refresh_after();
    harness.clock.set(due);
    harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
    assert_eq!(
        harness.calls("link"),
        0,
        "renewal does not request another permit"
    );
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
                let mut value =
                    serde_json::to_value(record.admission_cache.as_ref().unwrap()).unwrap();
                value["token"] = json!(membership_token(&grant));
                record.admission_cache = Some(serde_json::from_value(value).unwrap());
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
            assert_eq!(harness.calls("list"), 0);
            assert_eq!(harness.calls("renew"), 0);
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

#[tokio::test]
async fn account_mesh_terminal_renewal_denials_retire_authority_and_park() {
    for refusal in [
        RegistryRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        },
        RegistryRefusal::Forbidden,
        RegistryRefusal::Revoked,
    ] {
        let harness = leased(3600).await;
        harness.registry.script.lock().unwrap().endpoint_seconds = Some(600);
        *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
        harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
        assert_eq!(harness.state().publication, Publication::Published);
        let membership = harness.state().membership.clone().unwrap();
        let display = harness.state().self_installation.clone();
        harness.clock.set(membership.verified.refresh_after());
        harness.registry.script.lock().unwrap().membership = Some(refusal);
        let error = harness.mesh.refresh(EnrollTrigger::Lease).await;
        assert!(error.is_err(), "{refusal:?}");
        if refusal == RegistryRefusal::Revoked {
            assert_eq!(error, Err(AccountMeshRefusal::ReauthRequired));
        }
        assert!(harness.state().membership.is_none(), "{refusal:?}");
        assert_eq!(harness.state().self_installation, display);
        assert_eq!(harness.state().retry_at, None);
        assert_eq!(harness.state().publication, Publication::None);
        assert!(harness
            .transport
            .last
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .membership
            .is_none());
        assert_eq!(harness.mesh.lease_due(&error), None);
        assert!(harness
            .mesh
            .with(|inner| inner.record.admission_cache.is_none()));
        let calls = count(&harness.log, "registry:");
        harness.clock.set(START + 86400);
        for trigger in [
            EnrollTrigger::Automatic,
            EnrollTrigger::Network,
            EnrollTrigger::Lease,
        ] {
            assert!(harness.mesh.refresh(trigger).await.is_err());
        }
        assert_eq!(count(&harness.log, "registry:"), calls);
        harness.registry.script.lock().unwrap().membership = None;
        harness.registry.script.lock().unwrap().publication = None;
        if matches!(refusal, RegistryRefusal::MachineLimit { .. }) {
            harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
        } else {
            harness.mesh.enrollment_signed_in();
            harness
                .mesh
                .refresh(EnrollTrigger::Automatic)
                .await
                .unwrap();
        }
        assert!(harness.state().current_membership(START + 86400).is_some());
    }
}

#[tokio::test]
async fn account_mesh_terminal_limit_rearms_after_verified_plan_or_account_change() {
    for account_change in [false, true] {
        let harness = leased(3600).await;
        let due = harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .refresh_after();
        harness.clock.set(due);
        harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        });
        harness
            .mesh
            .refresh(EnrollTrigger::Lease)
            .await
            .unwrap_err();
        assert!(harness.state().plan_limit_locked());
        harness.registry.script.lock().unwrap().membership = None;
        if account_change {
            harness.sign_in("another-owner").await;
        } else {
            assert!(harness.mesh.machine_limit_changed());
            assert!(harness.state().plan_limit_locked());
        }
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        assert!(!harness.state().plan_limit_locked());
        assert!(harness.state().current_membership(due).is_some());
    }
}

#[tokio::test]
async fn account_mesh_terminal_limit_rearms_only_when_registry_observes_a_freed_slot() {
    let harness = leased(3600).await;
    let due = harness
        .state()
        .membership
        .as_ref()
        .unwrap()
        .verified
        .refresh_after();
    harness.clock.set(due);
    let refusal = RegistryRefusal::MachineLimit {
        limit: 2,
        active_count: 2,
    };
    harness.registry.script.lock().unwrap().membership = Some(refusal);
    harness
        .mesh
        .refresh(EnrollTrigger::Lease)
        .await
        .unwrap_err();
    harness.registry.script.lock().unwrap().membership = None;
    harness.registry.script.lock().unwrap().peer_presence = RegistryPresence::SignedOut;
    harness.mesh.list_fresh().await.unwrap();
    let calls = harness.calls("renew");
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(refusal.into())
    );
    assert_eq!(harness.calls("renew"), calls);
    harness.registry.script.lock().unwrap().peer_active = Some(false);
    harness.mesh.list_fresh().await.unwrap();
    assert!(harness.state().plan_limit_locked());
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert!(!harness.state().plan_limit_locked());
    assert!(harness.state().current_membership(due).is_some());
}

#[tokio::test]
async fn account_mesh_terminal_launch_denial_stays_parked_across_restart_until_rearmed() {
    for recovery in ["explicit", "plan", "signin"] {
        let mut harness = leased(86400).await;
        let due = harness
            .state()
            .membership
            .as_ref()
            .unwrap()
            .verified
            .refresh_after();
        harness.clock.set(due);
        let stale_cache = harness.store.record.lock().unwrap().admission_cache.clone();
        harness.restart().await;
        harness.sign_in("owner").await;
        let refusal = AccountMeshRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        };
        harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        });
        harness.log.lock().unwrap().clear();
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Automatic).await,
            Err(refusal)
        );
        assert_eq!(harness.calls("renew"), 1);
        assert_eq!(harness.calls("list"), 0);
        assert_eq!(harness.calls("sync"), 0);
        assert!(harness.state().membership.is_none());
        assert!(harness.mesh.with(|inner| !inner.launch_renewal_pending));
        let persisted = serde_json::to_vec(&*harness.store.record.lock().unwrap()).unwrap();
        *harness.store.record.lock().unwrap() = serde_json::from_slice(&persisted).unwrap();
        harness.store.record.lock().unwrap().admission_cache = stale_cache;
        harness.restart().await;
        harness.sign_in("owner").await;
        assert!(harness
            .store
            .record
            .lock()
            .unwrap()
            .admission_cache
            .is_none());
        harness.registry.script.lock().unwrap().membership = None;
        harness.log.lock().unwrap().clear();
        assert!(harness.mesh.with(|inner| inner.launch_renewal_pending));
        assert_eq!(
            harness.mesh.with(|inner| inner.membership_renewal_at()),
            None
        );
        for trigger in [
            EnrollTrigger::Automatic,
            EnrollTrigger::Lease,
            EnrollTrigger::Network,
        ] {
            assert_eq!(harness.mesh.refresh(trigger).await, Err(refusal));
        }
        assert_eq!(count(&harness.log, "registry:"), 0);
        assert_eq!(harness.mesh.lease_due(&Err(refusal)), None);
        assert!(harness.state().plan_limit_locked());
        let trigger = match recovery {
            "explicit" => EnrollTrigger::Explicit,
            "plan" => {
                assert!(harness.mesh.machine_limit_changed());
                EnrollTrigger::Automatic
            }
            "signin" => {
                harness.mesh.enrollment_signed_in();
                EnrollTrigger::Automatic
            }
            _ => unreachable!(),
        };
        harness.mesh.refresh(trigger).await.unwrap();
        assert_eq!(harness.calls("renew"), 1);
        assert!(harness.state().current_membership(due).is_some());
        assert!(harness
            .store
            .record
            .lock()
            .unwrap()
            .terminal_refusals
            .is_empty());
    }
}
