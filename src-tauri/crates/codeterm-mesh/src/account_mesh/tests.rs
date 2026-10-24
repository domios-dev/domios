mod refusals;
mod review;
mod support;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use support::*;

use super::gate::{self, BlockReason, TrafficDecision};
use super::ledger::{LinkAdmission, LinkLedger, LinkRecord, PeersLoad};
use super::link::{adopt_peer_with, link_installation_with};
use super::AdmissionRefusal;
use crate::types::RevocationList;

fn legacy_pair(a: &Machine, b: &Machine) {
    a.mesh.lock().insert_legacy_pair_for_test(b.as_peer());
    b.mesh.lock().insert_legacy_pair_for_test(a.as_peer());
}

fn admission(machine: &Machine, peer: &Machine) -> Option<LinkAdmission> {
    machine
        .mesh
        .lock()
        .link_record(&peer.id())
        .map(|record| record.admission.clone())
}

fn decision(machine: &Machine, peer: &Machine) -> TrafficDecision {
    machine.mesh.lock().traffic_decision(&peer.id())
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("account-mesh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn account_mesh_upgrade_snapshots_only_persisted_unrevoked_pairs_once() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    let peers: HashMap<_, _> = [b.as_peer(), c.as_peer()]
        .into_iter()
        .map(|peer| (peer.instance_id.clone(), peer))
        .collect();
    let mut revocations = RevocationList::default();
    revocations.revoked.push(c.id());

    let (ledger, persist) = LinkLedger::migrate(None, &peers, &revocations, PeersLoad::Present);
    assert!(persist, "the snapshot is persisted before gates open");
    assert_eq!(
        ledger.get(&b.id()).map(|r| &r.admission),
        Some(&LinkAdmission::Legacy {
            public_key: b.public_key()
        })
    );
    assert!(
        ledger.get(&c.id()).is_none(),
        "a revoked edge is never grandfathered"
    );

    let mut later = peers.clone();
    let newcomer = machine(4, &clock).as_peer();
    later.insert(newcomer.instance_id.clone(), newcomer.clone());
    let (reloaded, persist) =
        LinkLedger::migrate(Some(ledger), &later, &revocations, PeersLoad::Present);
    assert!(!persist);
    assert!(
        reloaded.get(&newcomer.instance_id).is_none(),
        "a record added after the snapshot never becomes legacy"
    );
    drop(a);

    let (empty, persist) =
        LinkLedger::migrate(None, &HashMap::new(), &revocations, PeersLoad::Held);
    assert!(
        empty.links.is_empty() && !persist,
        "a held file seeds nothing"
    );
}

#[test]
fn account_mesh_upgrade_and_restart_keep_keys_policy_and_traffic() {
    let dir = TempDir::new();
    let path = dir.0.join("peers.json");
    let clock = clock();
    let old = machine(2, &clock);
    let pre_upgrade = serde_json::json!({
        "peers": {
            old.id(): {
                "instance_id": old.id(),
                "instance_name": "old-peer",
                "addresses": [{"ip": old.ip, "is_tailscale": true}],
                "api_port": 7685,
                "public_key": old.public_key(),
                "paired_at": "2026-01-01T00:00:00Z",
                "last_seen": null,
                "access_level": "read_only",
                "version": null,
                "relay_protocol_version": crate::types::RELAY_PROTOCOL_VERSION
            }
        }
    });
    std::fs::write(&path, pre_upgrade.to_string()).unwrap();

    let first = crate::peers::load_mesh_records_at(&path);
    assert!(first.ledger.is_none(), "pre-upgrade file has no provenance");
    let (ledger, persist) = LinkLedger::migrate(
        first.ledger,
        &first.peers,
        &RevocationList::default(),
        first.load,
    );
    assert!(persist);
    crate::peers::save_peers_at(&path, &first.peers, &ledger).unwrap();

    let restarted = crate::peers::load_mesh_records_at(&path);
    let restored = restarted.ledger.expect("provenance survives restart");
    let peer = &restarted.peers[&old.id()];
    assert_eq!(peer.public_key, old.public_key(), "no key change");
    assert_eq!(peer.access_level, crate::types::AccessLevel::ReadOnly);
    let state = super::AccountMeshState::default();
    assert_eq!(
        gate::decide(
            restored.get(&old.id()),
            &peer.public_key,
            None,
            &state,
            START
        ),
        TrafficDecision::Open,
        "an upgraded legacy pair keeps working with no sign-in"
    );
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
fn account_mesh_sign_out_blocks_but_keeps_records_and_same_account_restores() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    assert!(
        decision(&a, &b).is_open(),
        "legacy edge stays open after sign-in"
    );
    assert_eq!(a.mesh.lock().owner_account(), Some(ACCOUNT));

    a.sign_out(2);
    assert_eq!(
        decision(&a, &b),
        TrafficDecision::Blocked(BlockReason::SignedOut)
    );
    {
        let manager = a.mesh.lock();
        assert!(
            manager.paired_peers.contains_key(&b.id()),
            "sign-out never unpairs"
        );
        assert!(manager.link_record(&b.id()).is_some());
        assert!(manager.known_public_keys().is_empty(), "inbound is blocked");
        assert!(manager.client_for(&b.id()).is_none(), "outbound is blocked");
        assert!(!manager.announce_allowed(&b.id()));
    }

    a.sign_in(3, OTHER_ACCOUNT, START + MEMBERSHIP_TTL);
    assert_eq!(
        decision(&a, &b),
        TrafficDecision::Blocked(BlockReason::OtherAccount),
        "a different account cannot resurrect the saved edge"
    );

    a.sign_in(4, ACCOUNT, START + MEMBERSHIP_TTL);
    assert!(
        decision(&a, &b).is_open(),
        "same account restores without re-pair"
    );
}

#[test]
fn account_mesh_adopted_edge_never_falls_back_and_expiry_keeps_the_record() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let remote_expiry = START + 600;
    {
        let mut manager = a.mesh.lock();
        let context = manager.admission_context().unwrap();
        manager
            .commit_local_adoption(
                &b.id(),
                &context,
                &b.installation_id,
                membership(ACCOUNT, &b.installation_id, remote_expiry),
            )
            .unwrap();
        assert_eq!(manager.admission_deadline(&b.id()), Some(remote_expiry));
    }
    assert!(decision(&a, &b).is_open());

    clock.store(remote_expiry, Ordering::SeqCst);
    assert_eq!(
        decision(&a, &b),
        TrafficDecision::Blocked(BlockReason::PeerMembershipExpired),
        "no grace at the original expiry"
    );
    assert!(
        matches!(admission(&a, &b), Some(LinkAdmission::Adopted { .. })),
        "expiry never downgrades to legacy"
    );
    assert!(a.mesh.lock().paired_peers.contains_key(&b.id()));
    assert!(
        a.mesh.lock().announce_allowed(&b.id()),
        "renewal can still be delivered"
    );

    let renewed = membership_token(ACCOUNT, &b.installation_id, remote_expiry + 1_800);
    assert!(a.mesh.lock().refresh_peer_membership(&b.id(), &renewed));
    assert!(decision(&a, &b).is_open(), "verified renewal reopens");

    let foreign = membership_token(OTHER_ACCOUNT, &b.installation_id, remote_expiry + 3_000);
    assert!(!a.mesh.lock().refresh_peer_membership(&b.id(), &foreign));
}

#[test]
fn account_mesh_known_local_revoke_blocks_immediately() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    let record = LinkRecord {
        link_nonce: None,
                admission: LinkAdmission::Adopted {
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
        plan_limit_locked: false,
    };
    assert!(gate::decide(Some(&record), &b.public_key(), Some(ACCOUNT), &state, START).is_open());
    state
        .revoked_installations
        .insert(b.installation_id.clone());
    assert_eq!(
        gate::decide(Some(&record), &b.public_key(), Some(ACCOUNT), &state, START),
        TrafficDecision::Blocked(BlockReason::Revoked)
    );
    assert_eq!(
        gate::decide(Some(&record), "rotated-key", Some(ACCOUNT), &state, START),
        TrafficDecision::Blocked(BlockReason::KeyMismatch)
    );
}

#[test]
fn account_mesh_state_change_retries_a_previously_refused_legacy_adoption() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let mut manager = a.mesh.lock();
    assert!(manager.begin_adoption(&b.id()));
    manager.finish_adoption(
        &b.id(),
        &Err(AdmissionRefusal::handshake("temporary refusal")),
    );
    assert!(!manager.begin_adoption(&b.id()));

    let mut changed = manager.account_state().clone();
    changed
        .revoked_installations
        .insert("revoked-installation".into());
    manager.apply_account_state(changed);
    assert!(
        manager.begin_adoption(&b.id()),
        "an updated account revocation state permits a fresh adoption attempt"
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
        .expect("same-account online link succeeds after receiver approval");
    assert_eq!(pair.instance_id, b.id());
    assert_eq!(pair.admission, "adopted");
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
        .expect("a repeated Add is idempotent");
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
async fn account_mesh_bilateral_adoption_commits_in_place() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    let key_before = a.mesh.lock().paired_peers[&b.id()].public_key.clone();

    adopt_peer_with(&a.mesh, &transport, &b.id())
        .await
        .expect("same-account legacy edge adopts");

    for (side, peer) in [(&a, &b), (&b, &a)] {
        assert!(matches!(
            admission(side, peer),
            Some(LinkAdmission::Adopted { .. })
        ));
        assert!(decision(side, peer).is_open(), "no drop across adoption");
        let manager = side.mesh.lock();
        assert_eq!(
            manager.paired_peers[&peer.id()].access_level,
            crate::types::AccessLevel::ReadOnly,
            "access policy is preserved"
        );
    }
    assert_eq!(a.mesh.lock().paired_peers[&b.id()].public_key, key_before);
    assert_eq!(a.authority.authorize_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn account_mesh_legacy_edge_of_another_owner_cannot_adopt() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    b.sign_in(1, OTHER_ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_out(2);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(3, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);

    assert!(adopt_peer_with(&a.mesh, &transport, &b.id()).await.is_err());
    assert!(matches!(
        admission(&a, &b),
        Some(LinkAdmission::Legacy { .. })
    ));
    assert!(matches!(
        admission(&b, &a),
        Some(LinkAdmission::Legacy { .. })
    ));
}

#[tokio::test]
async fn account_mesh_open_stream_ends_on_sign_out() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    let waiter = tokio::spawn(super::wait_until_blocked(a.mesh.clone(), b.id()));
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    a.sign_out(1);
    waiter.await.expect("the stream fence resolves on sign-out");
}

#[test]
fn account_mesh_unpair_removes_provenance_for_good() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    a.mesh.lock().unpair(&b.id()).unwrap();
    let mut manager = a.mesh.lock();
    assert!(manager.link_record(&b.id()).is_none());
    assert!(manager.is_revoked(&b.id()));
    manager.paired_peers.insert(b.id(), b.as_peer());
    assert_eq!(
        manager.traffic_decision(&b.id()),
        TrafficDecision::Blocked(BlockReason::NoProvenance),
        "a re-inserted record cannot regain the legacy exception"
    );
}

mod approval;
mod disconnect;
