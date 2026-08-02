use super::OrdinaryTabKey;
use super::{merge_ordinary_tab_projection, RemotePeerProjection};

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

#[test]
fn ordinary_tab_projection_merges_only_reachable_authenticated_peers() {
    let peer = |id: &str, authenticated, reachable| RemotePeerProjection {
        peer_id: id.into(),
        peer_name: id.to_uppercase(),
        authenticated,
        reachable,
        availability: "available",
        tabs: reachable.then(|| serde_json::json!({"id":"shared-tab", "title":id})).into_iter().collect(),
    };
    let projected = merge_ordinary_tab_projection(
        &[serde_json::json!({"id":"shared-tab", "title":"local"})],
        &[
            peer("a", true, true),
            peer("b", true, true),
            peer("offline", true, false),
            peer("unpaired", false, true),
        ],
    );
    let ids: Vec<_> = projected
        .iter()
        .filter_map(|tab| tab.get("id").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(ids, ["shared-tab", "mesh:a/shared-tab", "mesh:b/shared-tab"]);
}
