use super::*;
use serde_json::json;
#[test]
fn bounded_snapshots_redact_sensitive_subtrees_and_token_inputs() {
    let mut nodes: Vec<AxNode> = serde_json::from_value(json!([
 {"nodeId":"root","childIds":["secret","public"]},
 {"nodeId":"secret","parentId":"root","role":{"value":"textbox"},"name":{"value":"opaque-secret"},"value":{"value":"opaque-secret"},"backendDOMNodeId":2},
 {"nodeId":"public","parentId":"root","role":{"value":"button"},"name":{"value":"Continue"},"backendDOMNodeId":3}
 ])).unwrap();
    for attrs in [
        json!(["type", "password"]),
        json!(["name", "access_token"]),
        json!(["data-sensitive", ""]),
    ] {
        let mut sample = nodes.clone();
        redact(&mut sample,&json!({"root":{"backendNodeId":1,"children":[{"backendNodeId":2,"attributes":attrs}]}})).unwrap();
        let snapshot = super::super::build_snapshot_filtered(&sample, 1, "capture", true);
        assert!(snapshot.truncated);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.index.len(), 1);
        assert!(!snapshot.text.contains("opaque-secret"));
        assert_eq!(snapshot.items[0].name, "REDACTED");
    }
    assert!(redact(&mut nodes, &json!({})).is_err());
}

#[test]
fn bounded_ancestry_preserves_sensitive_shadow_and_frame_containment() {
    for attributes in [
        json!(["TyPe", "PaSsWoRd"]),
        json!(["NAME", "access_TOKEN"]),
        json!(["id", "mySecret"]),
        json!(["autocomplete", "ONE-TIME-CODE"]),
        json!(["name", "api-key"]),
        json!(["id", "apikey"]),
        json!(["data-sensitive", "false"]),
    ] {
        let mut dom = Dom::default();
        dom.record(&json!({"nodeId":1,"backendNodeId":1,"children":[
            {"nodeId":2,"backendNodeId":2,"attributes":attributes,"shadowRoots":[
                {"nodeId":3,"backendNodeId":3,"children":[{"nodeId":4,"backendNodeId":4,
                    "contentDocument":{"nodeId":5,"backendNodeId":5,"children":[{"nodeId":6,"backendNodeId":6}]}}]}
            ]},
            {"nodeId":7,"backendNodeId":7}
        ]}),None).unwrap();
        assert_eq!(dom.sensitive_ids(), HashSet::from([2, 3, 4, 5, 6]));
    }
}

#[test]
fn incomplete_and_cyclic_ancestry_is_sensitive_without_losing_public_nodes() {
    let mut dom = Dom::default();
    dom.record(&json!({"nodeId":1,"backendNodeId":1}), None)
        .unwrap();
    dom.record(&json!({"nodeId":2,"backendNodeId":2}), Some(99))
        .unwrap();
    dom.record(&json!({"nodeId":3,"backendNodeId":3}), Some(4))
        .unwrap();
    dom.record(&json!({"nodeId":4,"backendNodeId":4}), Some(3))
        .unwrap();
    assert_eq!(dom.sensitive_ids(), HashSet::from([2, 3, 4]));
}

#[test]
fn bounded_ancestry_redacts_ax_only_descendants_and_protected_properties() {
    let mut nodes: Vec<AxNode> = serde_json::from_value(json!([
        {"nodeId":"sensitive","backendDOMNodeId":1,"name":{"value":"secret"},"value":{"value":"secret"},
            "properties":[{"name":"description","value":{"value":"secret"}},{"name":"focused","value":{"value":true}}]},
        {"nodeId":"virtual","parentId":"sensitive","name":{"value":"secret"}},
        {"nodeId":"protected","name":{"value":"secret"},"properties":[{"name":"protected","value":{"value":true}}]},
        {"nodeId":"public","name":{"value":"Visible"}}
    ])).unwrap();
    redact_ids(&mut nodes, &HashSet::from([1]));
    for node in &nodes[..3] {
        assert_eq!(node.name.as_ref().unwrap().value, Some(json!("REDACTED")));
        assert!(node.value.is_none());
    }
    assert_eq!(nodes[0].properties.len(), 1);
    assert_eq!(nodes[0].properties[0].name, "focused");
    assert_eq!(
        nodes[3].name.as_ref().unwrap().value,
        Some(json!("Visible"))
    );
}
