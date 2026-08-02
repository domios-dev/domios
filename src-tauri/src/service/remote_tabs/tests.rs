use super::OrdinaryTabKey;

#[test]
fn remote_tab_identity_is_peer_and_tab_scoped() {
    let local = OrdinaryTabKey::local("shared-tab");
    let peer_a = OrdinaryTabKey::remote("peer-a", "shared-tab");
    let peer_b = OrdinaryTabKey::remote("peer-b", "shared-tab");
    let keys = std::collections::HashSet::from([local.clone(), peer_a.clone(), peer_b.clone()]);

    assert_eq!(keys.len(), 3);
    assert_ne!(peer_a, peer_b);
    assert_ne!(peer_a, local);
    assert_ne!(peer_b, local);
    assert_eq!(peer_a.ordinary_id(), "mesh:peer-a/shared-tab");

    for key in keys {
        let encoded = serde_json::to_string(&key).unwrap();
        assert_eq!(serde_json::from_str::<OrdinaryTabKey>(&encoded).unwrap(), key);
    }
}
