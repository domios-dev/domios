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
            Some(EndpointAddress::Tailnet {
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

#[tokio::test]
async fn endpoint_routing_caches_verified_records_and_rechecks_live_state() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    crate::account_mesh::link::link_installation_with(&a.mesh, &transport, &b.installation_id)
        .await
        .unwrap();
    let signed = record(&b, 1);
    a.authority.possession_checks.store(0, Ordering::SeqCst);
    a.mesh.lock().receive_endpoint_record(signed).unwrap();
    assert_eq!(a.authority.possession_checks.load(Ordering::SeqCst), 1);

    a.authority.possession_checks.store(0, Ordering::SeqCst);
    assert!(a.mesh.lock().client_for(&b.id()).is_some());
    assert!(a.mesh.lock().has_endpoint_record(&b.installation_id));
    assert!(a.mesh.lock().endpoint_target(&b.installation_id).is_some());
    assert_eq!(a.mesh.lock().endpoint_records().len(), 1);
    assert_eq!(a.authority.possession_checks.load(Ordering::SeqCst), 0);

    a.mesh
        .lock()
        .bind_account_authority(FakeAuthority::new(&a.installation_id));
    assert!(!a.mesh.lock().has_endpoint_record(&b.installation_id));
    assert!(a.mesh.lock().endpoint_target(&b.installation_id).is_none());
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

#[test]
fn direct_endpoint_changes_address_only_on_newer_signed_records() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    admit(&[&a, &b]);
    let token = record(&a, 1).claim.membership;
    let endpoint = EndpointAddress::DirectIngress {
        address: "8.8.8.8:7687".parse().unwrap(),
    };
    let record = a
        .mesh
        .lock()
        .sign_endpoint_record(Some(endpoint), 2, &token)
        .unwrap();
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(record.clone())
        .unwrap());
    assert!(b.mesh.lock().endpoint_target(&a.installation_id).is_none());
    let address = "1.1.1.1:7687".parse().unwrap();
    let update = a
        .mesh
        .lock()
        .sign_endpoint_record(Some(EndpointAddress::DirectIngress { address }), 3, &token)
        .unwrap();
    assert!(b.mesh.lock().receive_endpoint_record(update).unwrap());
    assert!(!b
        .mesh
        .lock()
        .receive_endpoint_record(record.clone())
        .unwrap());
    assert_eq!(
        b.mesh.lock().direct_ingress_address(&a.installation_id),
        Some(address)
    );
    let mut forged = record;
    forged.claim.endpoint = None;
    assert!(b.mesh.lock().verify_endpoint_record(&forged).is_err());
}

#[test]
fn provider_proxy_endpoint_round_trips_through_gossip_signing_and_refuses_invalid_hosts() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    admit(&[&a, &b, &c]);
    let token = record(&a, 1).claim.membership;
    let host = "domios-a1b2c3d4.fly.dev".to_owned();
    let signed = a
        .mesh
        .lock()
        .sign_endpoint_record(
            Some(EndpointAddress::ProviderProxy { host: host.clone() }),
            2,
            &token,
        )
        .unwrap();
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(signed.clone())
        .unwrap());
    for entry in b.mesh.lock().build_sync_payload().endpoints {
        c.mesh.lock().receive_endpoint_record(entry).unwrap();
    }
    let expected =
        codeterm_util::cloud_endpoint::CloudEndpoint::ProviderProxy { host: host.clone() };
    for peer in [&b, &c] {
        assert_eq!(
            peer.mesh.lock().cloud_ingress_endpoint(&a.installation_id),
            Some(expected.clone())
        );
        assert_eq!(
            peer.mesh.lock().direct_ingress_address(&a.installation_id),
            None
        );
        let status = peer.mesh.lock().peers_response();
        assert_eq!(
            status
                .direct_ingress
                .get(&a.installation_id)
                .map(|s| s.address.as_str()),
            Some("wss://domios-a1b2c3d4.fly.dev")
        );
    }

    let mut tampered = signed;
    tampered.claim.endpoint = Some(EndpointAddress::ProviderProxy {
        host: "other-host.fly.dev".into(),
    });
    assert!(b.mesh.lock().verify_endpoint_record(&tampered).is_err());

    for bad_host in [
        "127.0.0.1",
        "8.8.8.8",
        "App.Fly.Dev",
        "app.fly.dev:443",
        "::1",
    ] {
        assert!(a
            .mesh
            .lock()
            .sign_endpoint_record(
                Some(EndpointAddress::ProviderProxy {
                    host: bad_host.into(),
                }),
                3,
                &token,
            )
            .is_err());
    }
}

#[test]
fn provider_proxy_gossip_refuses_downgrade_to_direct_tailnet_withdrawal_and_host_change() {
    let clock = clock();
    let (a, b, c) = (machine(1, &clock), machine(2, &clock), machine(3, &clock));
    admit(&[&a, &b, &c]);
    let token = a
        .mesh
        .lock()
        .account_state()
        .membership
        .as_ref()
        .unwrap()
        .token
        .clone();
    let host = "domios-a1b2c3d4.fly.dev".to_owned();
    let original = a
        .mesh
        .lock()
        .sign_endpoint_record(
            Some(EndpointAddress::ProviderProxy { host: host.clone() }),
            1,
            &token,
        )
        .unwrap();
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(original.clone())
        .unwrap());

    // Helper to sign arbitrary newer claims with machine `a`'s valid installation authority.
    let sign_claim = |sequence: u64, endpoint: Option<EndpointAddress>| -> SignedEndpointRecord {
        let claim = crate::endpoint_gossip::EndpointClaim {
            version: crate::endpoint_gossip::ENDPOINT_RECORD_VERSION,
            installation_id: a.installation_id.clone(),
            sequence,
            membership: token.clone(),
            endpoint,
        };
        let signature = a
            .authority
            .sign_possession(&claim.digest().unwrap())
            .unwrap();
        SignedEndpointRecord { claim, signature }
    };

    // Downgrade to DirectIngress, Tailnet, withdrawal (None), or different host are all refused.
    let downgrade_direct = sign_claim(
        2,
        Some(EndpointAddress::DirectIngress {
            address: "8.8.8.8:7687".parse().unwrap(),
        }),
    );
    let downgrade_tailnet = sign_claim(
        3,
        Some(EndpointAddress::Tailnet {
            tailnet_ip: a.ip.clone(),
            api_port: a.port,
        }),
    );
    let withdrawal = sign_claim(4, None);
    let host_change = sign_claim(
        5,
        Some(EndpointAddress::ProviderProxy {
            host: "domios-hijacked.fly.dev".into(),
        }),
    );
    for forbidden in [
        &downgrade_direct,
        &downgrade_tailnet,
        &withdrawal,
        &host_change,
    ] {
        assert!(
            b.mesh
                .lock()
                .receive_endpoint_record(forbidden.clone())
                .is_err(),
            "gossip must refuse transition to {:?}",
            forbidden.claim.endpoint
        );
    }

    // Renewal with the exact same ProviderProxy host and higher sequence still succeeds.
    let same_host_renewal = sign_claim(
        6,
        Some(EndpointAddress::ProviderProxy { host: host.clone() }),
    );
    assert!(b
        .mesh
        .lock()
        .receive_endpoint_record(same_host_renewal)
        .unwrap());
    assert_eq!(
        b.mesh.lock().cloud_ingress_endpoint(&a.installation_id),
        Some(codeterm_util::cloud_endpoint::CloudEndpoint::ProviderProxy { host: host.clone() })
    );

    // Pinned managed endpoint on peer `c` refuses spoofed Direct or wrong-host gossip even before
    // any gossip record has ever been accepted for that installation.
    c.mesh.lock().pin_managed_cloud_endpoint(
        a.installation_id.clone(),
        codeterm_util::cloud_endpoint::CloudEndpoint::ProviderProxy { host: host.clone() },
    );
    assert!(c
        .mesh
        .lock()
        .receive_endpoint_record(downgrade_direct)
        .is_err());
    assert!(c.mesh.lock().receive_endpoint_record(host_change).is_err());
    assert!(c.mesh.lock().receive_endpoint_record(withdrawal).is_err());
    assert!(c.mesh.lock().receive_endpoint_record(original).unwrap());
    assert_eq!(
        c.mesh.lock().cloud_ingress_endpoint(&a.installation_id),
        Some(codeterm_util::cloud_endpoint::CloudEndpoint::ProviderProxy { host })
    );
}
