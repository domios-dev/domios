mod refusals;
mod review;
mod rotation;
mod support;

use super::gate::{self, BlockReason, TrafficDecision};
use super::ledger::{LinkAdmission, LinkLedger, LinkRecord};
use super::link::link_installation_with;
use super::AdmissionRefusal;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use support::*;

fn decision(machine: &Machine, peer: &Machine) -> TrafficDecision {
    machine.mesh.lock().traffic_decision(&peer.id())
}

#[test]
fn account_mesh_pair_without_provenance_is_blocked_everywhere() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    a.mesh.lock().paired_peers.insert(b.id(), b.as_peer());
    assert_eq!(
        decision(&a, &b),
        TrafficDecision::Blocked(BlockReason::NoProvenance)
    );
    let manager = a.mesh.lock();
    assert!(!manager.known_public_keys().contains_key(&b.id()));
    assert!(manager.client_for(&b.id()).is_none());
    assert!(manager.sign_request_for(&b.id()).is_err());
}

#[test]
fn rate_limited_admission_refusal_serializes_its_retry_delay() {
    assert_eq!(
        serde_json::to_value(AdmissionRefusal::RateLimited {
            retry_after_seconds: 60,
        })
        .unwrap(),
        serde_json::json!({
            "reason": "rate-limited",
            "retry_after_seconds": 60
        })
    );
}

#[test]
fn account_mesh_known_local_revoke_blocks_immediately() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    let record = LinkRecord {
        link_nonce: None,
        admission: LinkAdmission::Account {
            account_id: ACCOUNT.into(),
            installation_id: b.installation_id.clone(),
            public_key: b.public_key(),
        },
        remote_membership: Some(membership(ACCOUNT, &b.installation_id, START + 60)),
    };
    let mut state = super::AccountMeshState {
        account_epoch: 1,
        presence: super::AccountPresence::SignedIn {
            account_id: ACCOUNT.into(),
        },
        membership: Some(membership(ACCOUNT, &a.installation_id, START + 60)),
        revoked_installations: Default::default(),
        unavailable_installations: Default::default(),
        plan_limit_locked: false,
    };
    assert!(gate::decide(Some(&record), &b.public_key(), &state, START).is_open());
    state
        .revoked_installations
        .insert(b.installation_id.clone());
    assert_eq!(
        gate::decide(Some(&record), &b.public_key(), &state, START),
        TrafficDecision::Blocked(BlockReason::Revoked)
    );
    assert_eq!(
        gate::decide(Some(&record), "rotated-key", &state, START),
        TrafficDecision::Blocked(BlockReason::KeyMismatch)
    );
}

#[tokio::test]
async fn account_mesh_new_link_needs_both_fresh_permits_and_records_both_sides() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);

    let pair = link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .expect("same-account online link succeeds without per-edge approval");
    assert_eq!(pair.instance_id, b.id());
    assert_eq!(pair.admission, "account");
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 1);
    assert_eq!(b.authority.authorize_calls.load(Ordering::SeqCst), 1);
    assert!(decision(&a, &b).is_open());
    assert!(decision(&b, &a).is_open());
    assert_eq!(
        a.mesh.lock().paired_peers[&b.id()].public_key,
        b.public_key()
    );
    assert_eq!(
        b.mesh.lock().paired_peers[&a.id()].public_key,
        a.public_key()
    );

    let again = link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .expect("reconciliation is idempotent");
    assert_eq!(again.instance_id, b.id());
    assert_eq!(a.mesh.lock().paired_peers.len(), 1, "no duplicate peer");
}

#[tokio::test]
async fn account_mesh_offline_or_foreign_participant_cannot_link() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);

    b.authority.online.store(false, Ordering::SeqCst);
    let refused = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert_eq!(refused.unwrap_err(), AdmissionRefusal::Offline);
    assert!(
        a.mesh.lock().paired_peers.is_empty(),
        "initiator stored nothing"
    );
    assert!(
        b.mesh.lock().paired_peers.is_empty(),
        "responder stored nothing"
    );

    b.authority.online.store(true, Ordering::SeqCst);
    b.sign_in(2, OTHER_ACCOUNT, START + MEMBERSHIP_TTL);
    let foreign = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert!(matches!(
        foreign.unwrap_err(),
        AdmissionRefusal::HandshakeFailed { .. }
    ));
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());

    b.sign_out(3);
    let signed_out = link_installation_with(&b.mesh, &transport, &a.installation_id).await;
    assert_eq!(signed_out.unwrap_err(), AdmissionRefusal::SignedOut);
}

#[tokio::test]
async fn account_mesh_replayed_confirm_and_unregistered_address_cannot_link() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    let unregistered = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert_eq!(unregistered.unwrap_err(), AdmissionRefusal::PeerUnreachable);

    let forged = super::protocol::LinkConfirm {
        link_id: "never-offered".into(),
        installation_signature: "p|inst-1|00".into(),
        mesh_signature: "AAAA".into(),
        permit: "permit|inst-1|inst-2|account-a|00".into(),
    };
    assert!(b.mesh.lock().begin_link_confirm(&forged).is_err());
    assert!(b.mesh.lock().paired_peers.is_empty());
}

#[tokio::test]
async fn account_mesh_logout_during_handshake_persists_nothing() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let initiator = a.mesh.clone();
    let transport = Loopback::new(&[&a, &b]).after_each(Arc::new(move |path| {
        if path == super::protocol::LINK_HELLO_PATH {
            initiator
                .lock()
                .apply_account_state(super::AccountMeshState {
                    account_epoch: 2,
                    presence: super::AccountPresence::SignedOut,
                    membership: None,
                    revoked_installations: Default::default(),
                    unavailable_installations: Default::default(),
                    plan_limit_locked: false,
                });
        }
    }));

    let refused = link_installation_with(&a.mesh, &transport, &b.installation_id).await;
    assert_eq!(refused.unwrap_err(), AdmissionRefusal::SignedOut);
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
    assert!(a.mesh.lock().paired_peers.is_empty());
    assert!(b.mesh.lock().paired_peers.is_empty());
}

#[tokio::test]
async fn account_mesh_sign_out_expiry_and_foreign_account_never_open_a_connection() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id)
        .await
        .unwrap();
    a.sign_out(2);
    assert!(!decision(&a, &b).is_open());
    assert!(a.mesh.lock().paired_peers.contains_key(&b.id()));
    a.sign_in(3, ACCOUNT, START + MEMBERSHIP_TTL);
    assert!(decision(&a, &b).is_open());
    let mut blocked = Box::pin(super::wait_until_blocked(a.mesh.clone(), b.id()));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(blocked.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    clock.store(START + MEMBERSHIP_TTL, Ordering::SeqCst);
    a.mesh.lock().set_clock({
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    });
    blocked.await;
    assert!(!decision(&a, &b).is_open());
    a.sign_in(4, OTHER_ACCOUNT, START + MEMBERSHIP_TTL + 600);
    assert!(!decision(&a, &b).is_open());
}

#[tokio::test]
async fn account_mesh_one_connection_per_authenticated_installation() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(&a.mesh, &Loopback::new(&[&a, &b]), &b.installation_id)
        .await
        .unwrap();
    let party = super::transcript::Party {
        instance_id: c.id(),
        mesh_public_key: c.public_key(),
        installation_id: b.installation_id.clone(),
    };
    let descriptor = c.mesh.lock().local_descriptor(party);
    a.mesh
        .lock()
        .admit_new_link(
            &descriptor,
            &[],
            membership(ACCOUNT, &b.installation_id, START + MEMBERSHIP_TTL),
            ACCOUNT,
            "new-account-link",
        )
        .unwrap();
    let manager = a.mesh.lock();
    assert_eq!(manager.account_mesh_pairs().len(), 1);
    assert!(!manager.paired_peers.contains_key(&b.id()));
    assert!(manager.paired_peers.contains_key(&c.id()));
}

#[tokio::test]
async fn account_mesh_six_installations_connect_without_saved_pairs() {
    let clock = clock();
    let machines: Vec<_> = (1..=6).map(|index| machine(index, &clock)).collect();
    let refs: Vec<_> = machines.iter().collect();
    enroll_all(&refs);
    for machine in &machines {
        machine.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    }
    let transport = Arc::new(Loopback::new(&refs));
    let mut links = tokio::task::JoinSet::new();
    let mut selected = std::collections::BTreeSet::new();
    for source in &machines {
        for target in &machines {
            if source.installation_id == target.installation_id
                || !super::link::prefers_initiating(
                    &source.installation_id,
                    &target.installation_id,
                )
            {
                continue;
            }
            assert!(selected.insert((
                source.installation_id.clone(),
                target.installation_id.clone()
            )));
            let endpoint = source.authority.registry.lock()[&target.installation_id].clone();
            let mesh = source.mesh.clone();
            let transport = transport.clone();
            links.spawn(async move {
                super::link::connect_target_with(&mesh, transport.as_ref(), endpoint)
                    .await
                    .unwrap();
            });
        }
    }
    assert_eq!(selected.len(), 15);
    while let Some(result) = links.join_next().await {
        result.unwrap();
    }
    assert_eq!(
        machines
            .iter()
            .map(|machine| machine.authority.authorize_calls.load(Ordering::SeqCst))
            .sum::<usize>(),
        30
    );
    for machine in &machines {
        let manager = machine.mesh.lock();
        let connections = manager.account_mesh_pairs();
        let installations: std::collections::BTreeSet<_> = connections
            .iter()
            .map(|peer| peer.installation_id.clone())
            .collect();
        assert_eq!(connections.len(), 5);
        assert_eq!(installations.len(), 5);
        assert!(connections.iter().all(|peer| peer.admission == "account"
            && peer.online
            && manager.traffic_open(&peer.instance_id)));
        assert_eq!(machine.authority.authorize_calls.load(Ordering::SeqCst), 5);
    }
}
