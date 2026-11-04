use super::support::*;
use crate::account_mesh::AccountMeshAuthority;
use crate::endpoint_gossip::{EndpointAddress, SignedEndpointRecord};
use std::sync::atomic::Ordering;

fn admit(machines: &[&Machine]) {
    for machine in machines {
        machine.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
        let mut state = machine.mesh.lock().account_state().clone();
        state.registry_members = Some(
            machines
                .iter()
                .map(|peer| {
                    (
                        peer.installation_id.clone(),
                        (format!("key-{}", peer.installation_id), 1, 1),
                    )
                })
                .collect(),
        );
        machine.mesh.lock().apply_account_state(state);
    }
}

fn record(machine: &Machine, sequence: u64) -> SignedEndpointRecord {
    let mut manager = machine.mesh.lock();
    let membership = manager
        .account_state()
        .membership
        .as_ref()
        .unwrap()
        .token
        .clone();
    manager
        .sign_endpoint_record(
            Some(EndpointAddress {
                tailnet_ip: machine.ip.clone(),
                api_port: machine.port,
            }),
            sequence,
            &membership,
        )
        .unwrap()
}

#[test]
fn endpoint_gossip_accepts_signed_records_and_refuses_forgery_rollback_and_wrong_membership() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    admit(&[&a, &b, &c]);
    let old = record(&a, 1);
    let current = record(&a, 2);
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(current.clone())
        .unwrap());
    assert!(!b.mesh.lock().receive_endpoint_record(old).unwrap());
    assert!(!b
        .mesh
        .lock()
        .receive_endpoint_record(current.clone())
        .unwrap());
    let mut forged = current.clone();
    forged.claim.sequence = u64::MAX;
    assert!(b.mesh.lock().receive_endpoint_record(forged).is_err());
    let mut wrong_membership = current.clone();
    wrong_membership.claim.membership = c
        .mesh
        .lock()
        .account_state()
        .membership
        .as_ref()
        .unwrap()
        .token
        .clone();
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(wrong_membership)
        .is_err());
    let mut unsigned = current.clone();
    unsigned.signature.clear();
    assert!(b.mesh.lock().receive_endpoint_record(unsigned).is_err());
    let mut newer = current.clone();
    newer.claim.sequence = 3;
    newer.signature = c
        .authority
        .sign_possession(&newer.claim.digest().unwrap())
        .unwrap();
    assert!(b.mesh.lock().receive_endpoint_record(newer).is_err());
    assert_eq!(
        b.mesh
            .lock()
            .endpoint_target(&a.installation_id)
            .unwrap()
            .ip,
        a.ip
    );
    let mut state = b.mesh.lock().account_state().clone();
    state.registry_members = None;
    b.mesh.lock().apply_account_state(state);
    assert!(b.mesh.lock().endpoint_target(&a.installation_id).is_none());
    assert!(b.mesh.lock().receive_endpoint_record(current).is_err());
}

#[test]
fn endpoint_gossip_three_peers_converge_and_withdrawals_fence_old_records() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    admit(&[&a, &b, &c]);
    let original = record(&a, 1);
    b.mesh
        .lock()
        .receive_endpoint_record(original.clone())
        .unwrap();
    for entry in b.mesh.lock().build_sync_payload().endpoints {
        c.mesh.lock().receive_endpoint_record(entry).unwrap();
    }
    assert_eq!(
        c.mesh.lock().endpoint_records(),
        b.mesh.lock().endpoint_records()
    );
    let token = a
        .mesh
        .lock()
        .account_state()
        .membership
        .as_ref()
        .unwrap()
        .token
        .clone();
    let withdrawal = a.mesh.lock().sign_endpoint_record(None, 2, &token).unwrap();
    c.mesh.lock().receive_endpoint_record(withdrawal).unwrap();
    for entry in c.mesh.lock().build_sync_payload().endpoints {
        b.mesh.lock().receive_endpoint_record(entry).unwrap();
    }
    for peer in [&a, &b, &c] {
        assert!(peer.mesh.lock().has_endpoint_record(&a.installation_id));
        assert!(peer
            .mesh
            .lock()
            .endpoint_target(&a.installation_id)
            .is_none());
        assert!(!peer
            .mesh
            .lock()
            .receive_endpoint_record(original.clone())
            .unwrap());
    }
    clock.store(START + MEMBERSHIP_TTL, Ordering::SeqCst);
    assert!(b.mesh.lock().endpoint_records().is_empty());
    assert!(!b.mesh.lock().has_endpoint_record(&a.installation_id));
}

#[test]
fn endpoint_gossip_membership_revocation_and_key_changes_invalidate_cached_targets() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    admit(&[&a, &b]);
    let signed = record(&a, 1);
    b.mesh
        .lock()
        .receive_endpoint_record(signed.clone())
        .unwrap();
    let mut state = b.mesh.lock().account_state().clone();
    state
        .registry_members
        .as_mut()
        .unwrap()
        .get_mut(&a.installation_id)
        .unwrap()
        .1 = 2;
    b.mesh.lock().apply_account_state(state.clone());
    assert!(!b.mesh.lock().has_endpoint_record(&a.installation_id));
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(signed.clone())
        .is_err());
    state
        .registry_members
        .as_mut()
        .unwrap()
        .get_mut(&a.installation_id)
        .unwrap()
        .1 = 1;
    state
        .revoked_installations
        .insert(a.installation_id.clone());
    b.mesh.lock().apply_account_state(state);
    assert!(b.mesh.lock().endpoint_target(&a.installation_id).is_none());
    assert!(b.mesh.lock().receive_endpoint_record(signed).is_err());
}
