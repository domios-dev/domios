use super::*;

#[test]
fn account_mesh_rotation_membership_replaces_pin_without_expiry_or_generation_downgrade() {
    let old = membership(ACCOUNT, "installation-b", START + MEMBERSHIP_TTL);
    let mut ledger = LinkLedger::default();
    ledger.links.insert(
        "peer".into(),
        LinkRecord {
            link_nonce: None,
            admission: LinkAdmission::Adopted {
                account_id: ACCOUNT.into(),
                installation_id: "installation-b".into(),
                public_key: "transport-key".into(),
            },
            remote_membership: Some(old.clone()),
        },
    );
    let mut rotated = old.clone();
    rotated.verified.key_generation = 2;
    rotated.verified.key_id = "new-installation-key".into();
    assert!(ledger.refresh_remote_membership("peer", rotated.clone()));
    let mut downgrade = old;
    downgrade.verified.expires_at += 600;
    assert!(!ledger.refresh_remote_membership("peer", downgrade));
    let mut mismatch = rotated.clone();
    mismatch.verified.key_id = "other-installation-key".into();
    mismatch.verified.expires_at += 600;
    assert!(!ledger.refresh_remote_membership("peer", mismatch));
    assert_eq!(ledger.links["peer"].remote_membership, Some(rotated));
}

#[tokio::test]
async fn account_mesh_rotation_gate_restores_signed_requests_and_routing_without_restart() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    legacy_pair(&a, &b);
    a.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    b.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    let transport = Loopback::new(&[&a, &b]);
    adopt_peer_with(&a.mesh, &transport, &b.id()).await.unwrap();
    let mut state = a.mesh.lock().account_state().clone();
    state
        .revoked_installations
        .insert(b.installation_id.clone());
    a.mesh.lock().apply_account_state(state.clone());
    let signed = b.mesh.lock().sign_request().unwrap();
    assert!(a.mesh.lock().authenticate_request(&signed).is_none());
    assert!(a.mesh.lock().client_for(&b.id()).is_none());
    state.revoked_installations.clear();
    a.mesh.lock().apply_account_state(state);
    assert_eq!(a.mesh.lock().authenticate_request(&signed), Some(b.id()));
    assert!(a.mesh.lock().client_for(&b.id()).is_some());
    let (unknown_key, _) = crate::crown::generate_peer_keypair();
    let unknown = crate::crown::sign_request(&unknown_key, &b.id()).unwrap();
    assert!(a.mesh.lock().authenticate_request(&unknown).is_none());
}
