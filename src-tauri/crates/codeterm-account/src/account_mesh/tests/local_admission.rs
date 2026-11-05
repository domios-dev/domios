use super::support::*;
use crate::account_mesh::wire::*;
use crate::account_mesh::*;
use serde_json::json;

async fn admitted() -> Harness {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
}

fn peer_token() -> String {
    let mut payload = Issuer::membership_payload("owner", &"c".repeat(64), 1, 1, START);
    payload["installation_id"] = json!(PEER);
    membership_token(&Issuer::new().sign(&payload))
}

fn registry(
    harness: &Harness,
    revision: u64,
    revoked: bool,
) -> codeterm_entitlements::DetachedGrant {
    let mut rows = harness
        .state()
        .signed_registry
        .as_ref()
        .unwrap()
        .rows
        .clone();
    let peer = rows
        .iter_mut()
        .find(|row| row.installation_id == PEER)
        .unwrap();
    peer.active = !revoked;
    peer.eligibility = if revoked {
        Eligibility::Revoked
    } else {
        Eligibility::Eligible
    };
    Issuer::new().sign(&json!({
        "schema_version": 1, "iss": "domios-mesh-registry",
        "aud": {"project": PROJECT, "env": "test"}, "policy_epoch": EPOCH,
        "account_id": account_id("owner"), "revision": revision, "issued_at": START,
        "members": rows, "revoked_installations": if revoked {vec![PEER]} else {vec![]},
    }))
}

#[tokio::test]
async fn local_admission_retries_need_no_server_or_session_refresh() {
    let harness = admitted().await;
    let token = peer_token();
    let before = count(&harness.log, "registry:");
    harness.registry.script.lock().unwrap().challenge = Some(RegistryRefusal::Offline);
    for _ in 0..10 {
        let peer = harness.mesh.verify_peer_membership(&token, START).unwrap();
        assert_eq!(peer.installation_id, PEER);
        assert_eq!(
            harness
                .mesh
                .resolve_link_target(PEER)
                .await
                .unwrap()
                .target
                .installation_id,
            PEER
        );
        assert_eq!(harness.mesh.list().await.unwrap().registry_revision, 1);
    }
    assert_eq!(count(&harness.log, "registry:"), before);
    assert_eq!(
        harness.sessions.1.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn local_admission_refuses_expiry_forgery_scope_and_missing_signed_registry() {
    let harness = admitted().await;
    let token = peer_token();
    assert_eq!(
        harness.mesh.verify_peer_membership(&token, START + 3600),
        Err(MembershipRefusal::Expired)
    );
    let mut forged = parse_membership_token(&token).unwrap();
    forged.signature.replace_range(0..1, "!");
    assert!(harness
        .mesh
        .verify_peer_membership(&membership_token(&forged), START)
        .is_err());
    for (field, value) in [
        ("aud", json!({"project": PROJECT, "env": "live"})),
        ("sub", json!("foreign-account")),
        ("policy_epoch", json!("foreign-epoch")),
    ] {
        let mut payload = Issuer::membership_payload("owner", &"c".repeat(64), 1, 1, START);
        payload["installation_id"] = json!(PEER);
        payload[field] = value;
        let scoped = membership_token(&Issuer::new().sign(&payload));
        assert!(
            harness.mesh.verify_peer_membership(&scoped, START).is_err(),
            "{field}"
        );
    }
    harness
        .mesh
        .with(|inner| inner.update(&harness.mesh.watch, |state| state.signed_registry = None));
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    assert!(harness.state().current_membership(START).is_none());
    let calls = count(&harness.log, "registry:");
    assert_eq!(
        harness.mesh.resolve_link_target(PEER).await.unwrap_err(),
        AccountMeshRefusal::Unauthenticated
    );
    assert_eq!(count(&harness.log, "registry:"), calls);
}

#[tokio::test]
async fn local_admission_uses_highest_verified_revision_and_never_fetches_on_bad_gossip() {
    let harness = admitted().await;
    let token = peer_token();
    let before = count(&harness.log, "registry:");
    let older = registry(&harness, 20, false);
    let revoked = registry(&harness, 21, true);
    assert!(harness
        .mesh
        .receive_registry_snapshot(&revoked)
        .await
        .unwrap());
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    assert!(!harness
        .mesh
        .receive_registry_snapshot(&older)
        .await
        .unwrap());
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    let mut forged = registry(&harness, 22, false);
    forged.signature.replace_range(0..1, "!");
    assert!(harness
        .mesh
        .receive_registry_snapshot(&forged)
        .await
        .is_err());
    assert_eq!(
        harness.state().signed_registry.as_ref().unwrap().revision,
        21
    );
    assert_eq!(
        harness.mesh.resolve_link_target(PEER).await.unwrap_err(),
        AccountMeshRefusal::Revoked
    );
    assert_eq!(count(&harness.log, "registry:"), before);
}

#[tokio::test]
async fn local_revoke_blocks_before_failed_server_call_and_survives_signed_gossip() {
    let harness = admitted().await;
    let token = peer_token();
    let target = harness.mesh.resolve_link_target(PEER).await.unwrap().target;
    harness.registry.script.lock().unwrap().challenge = Some(RegistryRefusal::Offline);
    assert_eq!(
        harness.mesh.revoke(target).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
    assert_eq!(
        *harness.transport.revoked.lock().unwrap(),
        vec![PEER.to_owned()]
    );
    assert!(
        harness.store.record.lock().unwrap().local_revocations[&account_id("owner")].contains(PEER)
    );
    let not_yet_revoked = registry(&harness, 30, false);
    harness
        .mesh
        .receive_registry_snapshot(&not_yet_revoked)
        .await
        .unwrap();
    assert!(harness.mesh.verify_peer_membership(&token, START).is_err());
}

#[tokio::test]
async fn daily_membership_bundles_endpoint_and_only_one_authorization_pair() {
    for seconds in [3600, 86400, 172800] {
        let mut harness = Harness::new().await;
        harness.registry.script.lock().unwrap().membership_seconds = Some(seconds);
        *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
        harness.sign_in("owner").await;
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        let own = harness.state().membership.clone().unwrap();
        assert_eq!(own.verified.expires_at, START + seconds);
        assert_eq!(own.verified.refresh_after(), START + seconds - seconds / 12);
        assert_eq!(harness.calls("enroll"), 1);
        assert_eq!(harness.calls("sync"), 0);
        assert_eq!(harness.calls("publish"), 0);
        assert_eq!(harness.calls("list"), 0);
        harness.restart().await;
        harness.sign_in("owner").await;
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        assert_eq!(harness.calls("renew"), 0);
        harness.clock.set(own.verified.refresh_after());
        harness.mesh.refresh(EnrollTrigger::Lease).await.unwrap();
        assert_eq!(harness.calls("renew"), 1);
        assert_eq!(harness.calls("challenge"), 3);
        assert_eq!(harness.calls("sync"), 0);
        assert_eq!(harness.calls("publish"), 0);
        let requests = &harness.registry.script.lock().unwrap().renew_bodies;
        let request = serde_json::to_value(&requests[0]).unwrap();
        assert_eq!(request["own_row"], true);
        assert_eq!(request["endpoint"]["tailnet_ip"], concat!("100.", "64.1.2"));
        assert!(request.get("requested_membership_seconds").is_none());
    }
}

#[tokio::test]
async fn short_endpoint_lease_never_schedules_a_server_refresh() {
    let harness = admitted().await;
    let due = harness
        .state()
        .membership
        .as_ref()
        .unwrap()
        .verified
        .refresh_after();
    harness.mesh.with(|inner| {
        inner.published_endpoint = Some(tailnet_endpoint());
        inner.published_until = START + 600;
        inner.publication_refresh_after = START + 480;
    });
    assert_eq!(harness.mesh.lease_due(&Ok(())), Some(due));
}

#[tokio::test]
async fn transient_renewal_failure_keeps_membership_until_expiry_and_bounds_retry() {
    let harness = admitted().await;
    let membership = harness.state().membership.clone().unwrap();
    let due = membership.verified.refresh_after();
    harness.clock.set(due);
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Offline);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Lease).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(harness.state().membership.as_ref().unwrap(), &membership);
    assert!(harness.state().current_membership(due).is_some());
    let retry = harness.state().retry_at.unwrap();
    assert!(retry > due);
    assert!(retry <= due + (membership.verified.expires_at - due) / 2);
    assert!(harness
        .state()
        .current_membership(membership.verified.expires_at)
        .is_none());
}
