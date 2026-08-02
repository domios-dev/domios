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
        assert_eq!(
            serde_json::from_str::<OrdinaryTabKey>(&encoded).unwrap(),
            key
        );
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
        tabs: reachable
            .then(|| serde_json::json!({"id":"shared-tab", "title":id}))
            .into_iter()
            .collect(),
        controllers: std::collections::HashMap::from([("shared-tab".into(), "controller".into())]),
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
    assert_eq!(
        ids,
        ["shared-tab", "mesh:a/shared-tab", "mesh:b/shared-tab"]
    );
}

#[test]
fn remote_projection_uses_the_local_controller_context() {
    let projected = merge_ordinary_tab_projection(
        &[serde_json::json!({
            "id": "controller",
            "title": "Coordinator",
            "workspace_id": "workspace-a",
            "team_id": "team-a"
        })],
        &[RemotePeerProjection {
            peer_id: "peer-a".into(),
            peer_name: "REMOTE".into(),
            authenticated: true,
            reachable: true,
            availability: "available",
            tabs: vec![serde_json::json!({
                "id": "worker",
                "workspace_id": "remote-workspace",
                "team_id": "remote-team",
                "parent_tab_id": "remote-parent"
            })],
            controllers: std::collections::HashMap::from([("worker".into(), "controller".into())]),
        }],
    );
    let remote = &projected[1];
    assert_eq!(remote["parent_tab_id"], "controller");
    assert_eq!(remote["spawned_by_tab_id"], "controller");
    assert_eq!(remote["workspace_id"], "workspace-a");
    assert_eq!(remote["team_id"], "team-a");
    assert_eq!(remote["remote_controller_title"], "Coordinator");
}

#[test]
fn remote_projection_excludes_tabs_without_a_local_controller() {
    let projected = merge_ordinary_tab_projection(
        &[],
        &[RemotePeerProjection {
            peer_id: "peer-a".into(),
            peer_name: "REMOTE".into(),
            authenticated: true,
            reachable: true,
            availability: "available",
            tabs: vec![
                serde_json::json!({"id": "owned"}),
                serde_json::json!({"id": "foreign"}),
            ],
            controllers: std::collections::HashMap::from([("owned".into(), "controller".into())]),
        }],
    );
    let ids = projected
        .iter()
        .filter_map(|tab| tab.get("id").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(ids, ["mesh:peer-a/owned"]);
}
