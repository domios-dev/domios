use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const UID: &str = "user-1";

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
async fn account_mesh_sign_in_enrolls_once_and_restart_renews_the_same_slot() {
    let mut harness = enrolled().await;
    let state = harness.state();
    let membership = state.current_membership(START).expect("membership");
    assert_eq!(membership.verified.account_id, account_id(UID));
    assert_eq!(state.enrollment, Enrollment::Enrolled);
    assert_eq!(state.binding(), Binding::Current);
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.registry.slot(UID), Some(1));

    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("renew"), 0);

    harness.clock.set(START + 3600);
    harness.restart().await;
    assert_eq!(harness.state().presence, Presence::NeverSignedIn);
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("enroll_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.registry.slot(UID), Some(2));
    assert!(harness.state().current_membership(START + 3600).is_some());
}

#[tokio::test]
async fn account_mesh_unreadable_record_surfaces_a_refusal_and_keeps_sign_out_blocked() {
    let harness = Harness::new().await;
    *harness.store.corrupt.lock().unwrap() = true;
    harness.mesh.load().await;

    let state = harness.state();
    assert_eq!(state.presence, Presence::SignedOut);
    assert_eq!(state.enrollment, Enrollment::Refused);
    assert_eq!(state.last_refusal, Some(AccountMeshRefusal::Unavailable));
    assert_eq!(state.traffic(), Traffic::BlockedSignedOut);

    harness.sign_in(UID).await;
    assert_eq!(harness.state().enrollment, Enrollment::Refused);
    assert_eq!(
        harness.state().last_refusal,
        Some(AccountMeshRefusal::Unavailable)
    );
}

#[tokio::test]
async fn account_mesh_sign_out_returns_a_persistence_failure() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    *harness.store.fail_save.lock().unwrap() = true;

    let (_, persisted, released) = harness.mesh.sign_out(async { true }).await;

    assert_eq!(
        persisted,
        Err("Account Mesh sign-out could not be persisted".into())
    );
    assert!(released);
    assert_eq!(harness.state().presence, Presence::SignedOut);
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);
}

#[tokio::test]
async fn account_mesh_reconciles_lost_enroll_and_renew_replies_from_the_registry() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .lose_membership_reply = Some(RegistryRefusal::Offline);

    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.registry.slot(UID), Some(1));
    assert!(!harness
        .store
        .record
        .lock()
        .unwrap()
        .slots
        .contains_key(&account_id(UID)));

    harness.clock.set(harness.state().retry_at.unwrap());
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.registry.slot(UID), Some(2));
    assert_eq!(
        harness
            .store
            .record
            .lock()
            .unwrap()
            .slots
            .get(&account_id(UID)),
        Some(&2)
    );

    harness
        .registry
        .script
        .lock()
        .unwrap()
        .lose_membership_reply = Some(RegistryRefusal::Offline);
    let due = harness
        .state()
        .membership
        .as_ref()
        .unwrap()
        .verified
        .refresh_after();
    harness.clock.set(due);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.registry.slot(UID), Some(3));
    assert_eq!(
        harness
            .store
            .record
            .lock()
            .unwrap()
            .slots
            .get(&account_id(UID)),
        Some(&2)
    );

    harness.clock.set(harness.state().retry_at.unwrap());
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.registry.slot(UID), Some(3));
    assert_eq!(
        harness
            .store
            .record
            .lock()
            .unwrap()
            .slots
            .get(&account_id(UID)),
        Some(&3)
    );
    assert_eq!(harness.calls("recover_key"), 0);
    assert_eq!(harness.calls("enroll_key"), 1);
    let script = harness.registry.script.lock().unwrap();
    assert_eq!(script.renew_bodies.len(), 3);
    assert_eq!(script.renew_bodies[1], script.renew_bodies[2]);
}

#[tokio::test]
async fn account_mesh_recovers_when_the_enroll_key_reply_is_lost() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness.registry.script.lock().unwrap().lose_key_reply = Some(RegistryRefusal::Offline);

    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Explicit).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.calls("enroll_key"), 1);

    harness.clock.set(harness.state().retry_at.unwrap());
    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
    assert_eq!(harness.calls("enroll_key"), 2);
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.registry.slot(UID), Some(1));
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    assert_eq!(
        harness
            .store
            .record
            .lock()
            .unwrap()
            .keys
            .get(&account_id(UID))
            .map(|binding| binding.generation),
        Some(2)
    );
}

#[tokio::test]
async fn account_mesh_reenrolls_when_its_remote_row_is_inactive() {
    let harness = enrolled().await;
    harness.registry.revoke_self(UID);
    harness.clock.set(START + 3600);

    harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();

    assert_eq!(harness.calls("enroll"), 2);
    assert_eq!(harness.registry.slot(UID), Some(1));
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}

#[tokio::test]
async fn account_mesh_recovers_a_memory_only_key_after_restart_and_renews() {
    let mut harness = enrolled().await;
    let old_binding = harness
        .store
        .record
        .lock()
        .unwrap()
        .keys
        .get(&account_id(UID))
        .cloned()
        .unwrap();
    let seed = harness.keys.0.lock().unwrap().clone().unwrap();
    assert!(
        !serde_json::to_string(&*harness.store.record.lock().unwrap())
            .unwrap()
            .contains(seed.expose())
    );

    *harness.keys.0.lock().unwrap() = None;
    harness.restart().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();

    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.registry.slot(UID), Some(2));
    let state = harness.state();
    let recovered = state.self_installation.as_ref().unwrap();
    assert_ne!(recovered.key_id, old_binding.key_id);
    assert_eq!(recovered.key_generation, old_binding.generation + 1);
    assert_eq!(recovered.slot_revision, 2);
}

#[tokio::test]
async fn account_mesh_publishes_only_a_ready_listener_and_withdraws_when_it_goes_away() {
    let harness = enrolled().await;
    assert_eq!(harness.state().publication, Publication::NotReady);
    assert_eq!(harness.calls("publish"), 0);

    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(
        harness.calls("renew"),
        0,
        "a valid membership is not renewed on network events"
    );
    assert_eq!(
        harness.calls("list"),
        0,
        "sync supplies metadata without a separate list"
    );
    assert_eq!(harness.calls("publish"), 1);
    assert_eq!(harness.state().publication, Publication::Published);
    assert_eq!(
        harness.registry.endpoint(UID).unwrap().tailnet_ip,
        "100.64.1.2"
    );
    assert!(harness.store.record.lock().unwrap().published);

    *harness.endpoint.0.lock().unwrap() = None;
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("withdraw"), 1);
    assert_eq!(harness.calls("list"), 0);
    assert!(harness.registry.endpoint(UID).is_none());
    assert!(!harness.store.record.lock().unwrap().published);
}

#[tokio::test]
async fn account_mesh_concurrent_triggers_share_one_flight() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().gate = Some(gate.clone());
    let first = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Ok(()));
    harness.registry.script.lock().unwrap().gate = None;
    gate.notify_one();
    assert_eq!(first.await.unwrap(), Ok(()));
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(
        harness.calls("renew"),
        0,
        "joined triggers reconcile the listener without renewing again"
    );
}

#[tokio::test]
async fn account_mesh_sign_out_blocks_durably_before_one_withdrawal_and_keeps_pairing_state() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    let slots = harness.store.record.lock().unwrap().slots.clone();
    harness.log.lock().unwrap().clear();

    let cleanup = harness.logout().await;
    assert_eq!(cleanup, Cleanup::Confirmed);
    let blocked = position(&harness.log, "save:signed_out=true");
    let notified = position(&harness.log, "transport:signed_out");
    let withdrawn = position(&harness.log, "registry:withdraw:token-user-1");
    assert!(notified < blocked && blocked < withdrawn);
    assert_eq!(harness.calls("withdraw"), 1);
    assert_eq!(
        harness.calls("revoke"),
        0,
        "sign-out never releases the durable slot"
    );
    assert!(harness.transport.revoked.lock().unwrap().is_empty());

    let state = harness.state();
    assert_eq!(state.traffic(), Traffic::BlockedSignedOut);
    assert_eq!(state.cleanup, Cleanup::Confirmed);
    assert!(state.membership.is_none());
    let record = harness.store.record.lock().unwrap().clone();
    assert!(record.signed_out && !record.cleanup_pending && !record.published);
    assert_eq!(record.slots, slots);
    assert_eq!(record.slots.get(&account_id(UID)), Some(&1));
    assert_eq!(record.owner, Some(account_id(UID)));
    assert!(harness.registry.endpoint(UID).is_none());
}

#[tokio::test]
async fn account_mesh_failed_withdrawal_is_pending_and_retains_no_token() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::Offline);

    assert_eq!(harness.logout().await, Cleanup::Pending);
    assert_eq!(harness.calls("withdraw"), 1);
    assert_eq!(harness.state().cleanup, Cleanup::Pending);
    assert!(harness.store.record.lock().unwrap().cleanup_pending);

    let before = harness.log.lock().unwrap().len();
    for trigger in [
        EnrollTrigger::Lease,
        EnrollTrigger::Network,
        EnrollTrigger::Explicit,
    ] {
        assert_eq!(
            harness.mesh.refresh(trigger).await,
            Err(AccountMeshRefusal::SignedOut)
        );
    }
    assert_eq!(
        harness.mesh.list().await.unwrap_err(),
        AccountMeshRefusal::SignedOut
    );
    assert_eq!(
        harness.log.lock().unwrap().len(),
        before,
        "no registry call after logout"
    );
}

#[tokio::test]
async fn account_mesh_resumed_publication_or_withdrawal_clears_pending_cleanup_durably() {
    for publish in [true, false] {
        let mut harness = enrolled().await;
        *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
        harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
        harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::Offline);
        assert_eq!(harness.logout().await, Cleanup::Pending);
        harness.restart().await;
        harness.sign_in(UID).await;
        if !publish {
            *harness.endpoint.0.lock().unwrap() = None;
        }
        assert_eq!(harness.state().cleanup, Cleanup::Pending);
        harness
            .mesh
            .refresh(EnrollTrigger::Explicit)
            .await
            .unwrap_err();
        assert_eq!(harness.state().cleanup, Cleanup::Pending);
        assert!(harness.store.record.lock().unwrap().cleanup_pending);
        harness.registry.script.lock().unwrap().publication = None;
        harness.clock.set(START + 60);
        harness.mesh.refresh(EnrollTrigger::Explicit).await.unwrap();
        assert_eq!(harness.state().cleanup, Cleanup::Confirmed);
        assert!(!harness.store.record.lock().unwrap().cleanup_pending);
        assert_eq!(
            harness.state().publication,
            if publish {
                Publication::Published
            } else {
                Publication::NotReady
            }
        );
        harness.restart().await;
        assert_eq!(harness.state().cleanup, Cleanup::None);
    }
}

#[tokio::test]
async fn account_mesh_logout_during_enrollment_cannot_be_reopened_by_the_late_response() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().gate = Some(gate.clone());
    let flight = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;

    let racing_identity = identity(UID);
    let racing_refresh = harness.mesh.identity_changed(Some(&racing_identity));
    assert_eq!(
        harness.mesh.sign_out(racing_refresh).await.0,
        Cleanup::Confirmed
    );
    gate.notify_one();
    assert_eq!(flight.await.unwrap(), Err(AccountMeshRefusal::Cancelled));
    let state = harness.state();
    assert_eq!(
        state.presence,
        Presence::SignedOut,
        "a racing token refresh is not a sign-in"
    );
    assert!(state.membership.is_none());
    assert_eq!(state.enrollment, Enrollment::Idle);
}

#[tokio::test]
async fn account_mesh_same_account_resumes_and_another_account_cannot() {
    let harness = enrolled().await;
    let signed_out_epoch = {
        harness.logout().await;
        harness.state().account_epoch
    };
    assert_eq!(harness.state().binding(), Binding::Current);

    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let resumed = harness.state();
    assert!(resumed.account_epoch > signed_out_epoch);
    assert_eq!(resumed.traffic(), Traffic::Open);
    assert_eq!(resumed.binding(), Binding::Current);
    assert!(resumed.current_membership(START).is_some());
    assert_eq!(
        harness.calls("enroll"),
        1,
        "same account renews its slot, no re-pair"
    );
    assert!(harness.transport.revoked.lock().unwrap().is_empty());

    harness.logout().await;
    harness.sign_in("user-2").await;
    let other = harness.state();
    assert_eq!(other.binding(), Binding::Other);
    assert_eq!(other.owner, Some(account_id(UID)));
    assert_eq!(
        other.signed_in_account(),
        Some(account_id("user-2").as_str())
    );
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let membership = harness.state().current_membership(START).cloned().unwrap();
    assert_ne!(membership.verified.account_id, account_id(UID));
    assert_eq!(
        harness.calls("enroll_key"),
        2,
        "the key is bound per account"
    );

    harness.logout().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(
        harness.calls("enroll_key"),
        2,
        "switching back reuses the account's key"
    );
    assert_eq!(harness.state().binding(), Binding::Current);
}

#[tokio::test]
async fn account_mesh_account_switch_bumps_the_epoch_and_drops_membership() {
    let harness = enrolled().await;
    let before = harness.state().account_epoch;
    harness.sign_in("user-2").await;
    let state = harness.state();
    assert_eq!(state.account_epoch, before + 1);
    assert!(state.membership.is_none());
    assert_eq!(state.traffic(), Traffic::Open);
    let last = harness.transport.last.lock().unwrap().clone().unwrap();
    assert_eq!(last.account_epoch, state.account_epoch);
}

#[tokio::test]
async fn account_mesh_upgrade_without_account_is_not_a_sign_out() {
    let harness = Harness::new().await;
    assert_eq!(harness.state().presence, Presence::NeverSignedIn);
    assert_eq!(harness.state().traffic(), Traffic::Open);
    assert_eq!(harness.mesh.sign_out(async {}).await.0, Cleanup::None);
    harness.mesh.identity_changed(None).await;
    assert_eq!(harness.state().presence, Presence::NeverSignedIn);
    assert!(!harness.store.record.lock().unwrap().signed_out);
}

#[tokio::test]
async fn account_mesh_signed_out_state_survives_restart_and_unreadable_state_fails_closed() {
    let mut harness = enrolled().await;
    harness.logout().await;
    harness.restart().await;
    assert_eq!(harness.state().presence, Presence::SignedOut);
    assert_eq!(harness.state().traffic(), Traffic::BlockedSignedOut);

    let corrupt = Harness::build(Some(Issuer::new().trust()), true).await;
    assert_eq!(corrupt.state().presence, Presence::SignedOut);
}

#[tokio::test]
async fn account_mesh_revoke_releases_the_slot_and_delegates_pairing_removal() {
    let harness = enrolled().await;
    let target = wire::MeshTarget {
        installation_id: PEER.into(),
        key_id: "c".repeat(64),
        key_generation: 1,
        slot_revision: 1,
    };
    let revoked = harness.mesh.revoke(target).await.unwrap();
    assert_eq!(revoked.slot_revision, 2);
    assert_eq!(
        *harness.transport.revoked.lock().unwrap(),
        vec![PEER.to_owned()]
    );
    assert!(harness.state().revoked_installations.contains(PEER));
    assert!(harness.state().current_membership(START).is_some());
}

#[tokio::test]
async fn account_mesh_session_revoke_uses_a_signed_request_and_keeps_the_pairing() {
    let harness = enrolled().await;
    let before = harness.state();
    let revoked = harness.mesh.revoke_session(PEER.into()).await.unwrap();
    assert_eq!(revoked.target_installation_id, PEER);
    assert_eq!(revoked.revoked_at, START);
    assert_eq!(harness.calls("session_revoke"), 1);
    assert_eq!(
        *harness.transport.revoked.lock().unwrap(),
        vec![PEER.to_owned()]
    );
    assert_eq!(harness.state().self_installation, before.self_installation);
    assert!(harness.state().current_membership(START).is_some());
}

#[tokio::test]
async fn account_mesh_new_links_need_the_online_permit_and_the_registry_address() {
    let harness = enrolled().await;
    let target = harness.mesh.resolve_link_target(PEER).await.unwrap();
    assert_eq!(target.endpoint.ip().to_string(), "100.101.102.103");
    let transcript = [6u8; 32];
    let request = LinkAuthorization {
        transcript,
        local_installation_id: INSTALLATION.into(),
        peer_installation_id: PEER.into(),
        account_id: account_id(UID),
    };
    let permit = harness.mesh.authorize_link(request.clone()).await.unwrap();
    assert_eq!(harness.calls("link"), 1);
    let grant = parse_membership_token(&permit).unwrap();
    let peer_view = LinkPermitExpectation {
        transcript: &transcript,
        holder_installation_id: INSTALLATION,
        peer_installation_id: PEER,
        account_id: &request.account_id,
        peer_target: Some(&target.target),
    };
    assert_eq!(
        verify_link_permit(&grant, &Issuer::new().trust(), &peer_view, START),
        Ok(())
    );

    harness.registry.script.lock().unwrap().permit = Some(RegistryRefusal::Unavailable);
    assert!(harness.state().current_membership(START).is_some());
    assert_eq!(
        harness.mesh.authorize_link(request.clone()).await,
        Err(AccountMeshRefusal::Unavailable),
        "a cached membership never authorizes a new link"
    );

    harness.clock.set(START + 3600);
    assert_eq!(
        harness.mesh.authorize_link(request).await,
        Err(AccountMeshRefusal::Revoked)
    );
}

#[tokio::test]
async fn account_mesh_responder_authorizes_an_initiator_without_a_published_endpoint() {
    let harness = enrolled().await;
    harness.registry.script.lock().unwrap().peer_unpublished = true;
    assert_eq!(
        harness.mesh.resolve_link_target(PEER).await.err(),
        Some(AccountMeshRefusal::Unavailable),
        "dialing still needs the published endpoint"
    );
    let request = LinkAuthorization {
        transcript: [7u8; 32],
        local_installation_id: INSTALLATION.into(),
        peer_installation_id: PEER.into(),
        account_id: account_id(UID),
    };
    assert!(harness.mesh.authorize_link(request).await.is_ok());
    assert_eq!(harness.calls("link"), 1);
}

#[tokio::test]
async fn account_mesh_peer_membership_and_possession_follow_the_signed_in_account() {
    let harness = enrolled().await;
    let state = harness.state();
    let own = state.current_membership(START).unwrap();
    let verified = harness
        .mesh
        .verify_peer_membership(&own.token, START)
        .unwrap();
    assert_eq!(verified, own.verified);
    let transcript = [2u8; 32];
    let possession = harness.mesh.sign_possession(&transcript).unwrap();
    assert!(harness
        .mesh
        .verify_possession(&verified, &transcript, &possession));

    harness.logout().await;
    assert_eq!(
        harness.mesh.sign_possession(&transcript),
        Err(AccountMeshRefusal::SignedOut)
    );
    assert_eq!(
        harness.mesh.verify_peer_membership(&own.token, START),
        Err(MembershipRefusal::Subject)
    );
}

#[tokio::test]
async fn account_mesh_rename_and_list_are_observations() {
    let harness = enrolled().await;
    let observation = harness.mesh.list().await.unwrap();
    assert_eq!(observation.rows.len(), 2);
    assert_eq!(harness.state().registry.as_ref(), Some(&observation));
    let updated = harness.mesh.rename_self(0, "Desk".into()).await.unwrap();
    assert_eq!(updated.metadata_revision, 1);
    let own = harness.state().self_installation.clone().unwrap();
    assert_eq!(
        (own.metadata_revision, own.display_name.as_deref()),
        (1, Some("Desk"))
    );
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}

#[tokio::test]
async fn account_mesh_internal_registry_reads_update_the_snapshot() {
    let harness = enrolled().await;
    assert!(harness.state().registry.is_some());

    harness.mesh.resolve_link_identity(PEER).await.unwrap();

    let observation = harness
        .state()
        .registry
        .clone()
        .expect("stored registry read");
    assert_eq!(observation.rows.len(), 2);
    assert_eq!(observation.rows[0].installation_id, INSTALLATION);
}

#[tokio::test]
async fn account_mesh_clock_behind_the_issuer_still_enrolls_without_a_longer_lease() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    harness.registry.script.lock().unwrap().issuer_ahead = 2;
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await, Ok(()));
    let state = harness.state();
    let membership = state
        .current_membership(START)
        .expect("a fresh membership is current on a clock 2 s behind");
    assert_eq!(membership.verified.issued_at, START + 2);
    assert!(
        state.current_membership(START + 2 + 3600).is_none(),
        "expiry is never extended"
    );
}

async fn next_renewal(harness: &Harness) -> Duration {
    let state = harness.state();
    let due = state
        .retry_at
        .unwrap_or_else(|| state.membership.as_ref().unwrap().verified.refresh_after());
    let previous = harness.mesh.now();
    harness.clock.set(due);
    tokio::time::timeout(
        Duration::from_secs(3600),
        harness.registry.entered.notified(),
    )
    .await
    .expect("a renewal is scheduled");
    Duration::from_secs(due - previous)
}

#[tokio::test(start_paused = true)]
async fn account_mesh_failed_lease_renewal_retries_with_bounded_backoff() {
    let harness = Harness::with_leases().await;
    harness.sign_in(UID).await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.entered.notified().await;
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);

    assert_eq!(next_renewal(&harness).await, Duration::from_secs(3000));
    assert_eq!(harness.state().enrollment, Enrollment::Refused);
    assert_eq!(
        next_renewal(&harness).await,
        Duration::from_secs(30),
        "a failed renewal re-arms before expiry"
    );
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::RateLimited {
        retry_after_seconds: 900,
    });
    assert_eq!(
        next_renewal(&harness).await,
        Duration::from_secs(60),
        "backoff grows"
    );
    harness.registry.script.lock().unwrap().membership = None;
    assert_eq!(
        next_renewal(&harness).await,
        Duration::from_secs(900),
        "the server's retry-after is honoured"
    );
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
    assert_eq!(harness.calls("renew"), 4);
    assert_eq!(
        next_renewal(&harness).await,
        Duration::from_secs(3000),
        "a renewed membership returns to the lease cadence"
    );
}

#[tokio::test]
async fn account_mesh_explicit_enroll_answers_with_the_run_it_joins() {
    let harness = Harness::new().await;
    harness.sign_in(UID).await;
    let gate = Arc::new(Notify::new());
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.gate = Some(gate.clone());
        script.membership = Some(RegistryRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        });
    }
    let first = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.refresh(EnrollTrigger::Automatic).await }
    });
    harness.registry.entered.notified().await;
    let explicit = harness.mesh.refresh(EnrollTrigger::Explicit);
    tokio::pin!(explicit);
    assert!(
        tokio::time::timeout(Duration::ZERO, &mut explicit)
            .await
            .is_err(),
        "an explicit enroll does not answer before the flight it joined settles"
    );
    harness.registry.script.lock().unwrap().gate = None;
    gate.notify_one();

    let refusal = AccountMeshRefusal::MachineLimit {
        limit: 2,
        active_count: 2,
    };
    assert_eq!(explicit.await, Err(refusal));
    assert_eq!(first.await.unwrap(), Err(refusal));
    assert_eq!(
        harness.calls("enroll"),
        2,
        "the explicit trigger reran once"
    );
    let notice = harness
        .state()
        .limit_notice
        .clone()
        .expect("the explicit refusal carries its notice");
    assert_eq!(notice.trigger, EnrollTrigger::Explicit);
    assert_eq!(notice.refusal, refusal);
}

#[tokio::test]
async fn account_mesh_abandoned_logout_stays_blocked_until_the_next_sign_in() {
    let harness = enrolled().await;
    let mut states = harness.mesh.subscribe();
    let logout = tokio::spawn({
        let mesh = harness.mesh.clone();
        async move { mesh.sign_out(std::future::pending::<()>()).await }
    });
    states
        .wait_for(|state| state.presence == Presence::SignedOut)
        .await
        .unwrap();
    logout.abort();
    assert!(logout.await.unwrap_err().is_cancelled());
    assert_eq!(
        harness.state().traffic(),
        Traffic::BlockedSignedOut,
        "an abandoned logout fails closed"
    );

    harness.mesh.identity_changed(Some(&identity(UID))).await;
    assert_eq!(
        harness.state().signed_in_account(),
        Some(account_id(UID).as_str()),
        "a later sign-in reopens Mesh"
    );
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Explicit).await, Ok(()));
}

#[tokio::test]
async fn account_mesh_publication_retry_preserves_reason_and_success() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    let refusal = AccountMeshRefusal::RateLimited {
        retry_after_seconds: 7,
    };
    harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::RateLimited {
        retry_after_seconds: 7,
    });
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(refusal)
    );
    assert_eq!(harness.state().publication_failure, Some(refusal));
    assert_eq!(harness.state().retry_at, Some(START + 30));
    for trigger in [
        EnrollTrigger::Network,
        EnrollTrigger::Explicit,
        EnrollTrigger::Automatic,
    ] {
        assert_eq!(harness.mesh.refresh(trigger).await, Err(refusal));
    }
    assert_eq!(harness.calls("publish"), 1);
    harness.clock.set(START + 30);
    harness.registry.script.lock().unwrap().publication = None;
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await, Ok(()));
    assert_eq!(harness.calls("publish"), 2);
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(harness.state().publication, Publication::Published);
    assert_eq!(harness.state().publication_failure, None);
    assert_eq!(harness.state().retry_at, None);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("publish"), 2);
    harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::Unavailable);
    *harness.endpoint.0.lock().unwrap() = None;
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Network).await,
        Err(AccountMeshRefusal::Unavailable)
    );
    harness.logout().await;
    assert_eq!(harness.state().retry_at, None);
    assert_eq!(harness.state().publication_failure, None);
}

#[tokio::test]
async fn account_mesh_refused_read_keeps_successful_publication() {
    let harness = enrolled().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.state().publication, Publication::Published);
    harness.registry.script.lock().unwrap().list = Some(RegistryRefusal::RateLimited {
        retry_after_seconds: 60,
    });
    assert_eq!(
        harness.mesh.list().await.unwrap_err(),
        AccountMeshRefusal::RateLimited {
            retry_after_seconds: 60
        }
    );
    assert_eq!(harness.state().publication, Publication::Published);
    assert_eq!(harness.state().publication_failure, None);
    assert_eq!(harness.calls("publish"), 1);
}

#[tokio::test(start_paused = true)]
async fn account_mesh_initial_outage_schedules_one_retry_without_trigger_bursts() {
    let harness = Harness::with_leases().await;
    harness.sign_in(UID).await;
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Unavailable)
    );
    harness.registry.entered.notified().await;
    for trigger in [
        EnrollTrigger::Network,
        EnrollTrigger::Explicit,
        EnrollTrigger::Automatic,
    ] {
        assert_eq!(
            harness.mesh.refresh(trigger).await,
            Err(AccountMeshRefusal::Unavailable)
        );
    }
    assert_eq!(harness.calls("enroll"), 1);
    harness.registry.script.lock().unwrap().membership = None;
    assert_eq!(next_renewal(&harness).await, Duration::from_secs(30));
    assert_eq!(harness.calls("enroll"), 2);
    assert_eq!(harness.state().enrollment, Enrollment::Enrolled);
}
