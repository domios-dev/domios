use super::*;
use serde_json::json;
#[test]
fn scoped_locators_refuse_ambiguity_and_exclude_other_dialogs() {
    let nodes: Vec<AxNode> = serde_json::from_value(json!([
      {"nodeId":"root","childIds":["a","b"]},
      {"nodeId":"a","parentId":"root","role":{"value":"dialog"},"name":{"value":"One"},"childIds":["x"]},
      {"nodeId":"b","parentId":"root","role":{"value":"dialog"},"name":{"value":"Two"},"childIds":["y"]},
      {"nodeId":"x","parentId":"a","role":{"value":"button"},"name":{"value":"Save"},"backendDOMNodeId":7},
      {"nodeId":"y","parentId":"b","role":{"value":"button"},"name":{"value":"Save"},"backendDOMNodeId":8}
    ])).unwrap();
    assert!(unique(&nodes, "button", Some("Save"))
        .unwrap_err()
        .starts_with("ambiguous:"));
    assert!(scoped(&nodes, "dialog")
        .unwrap_err()
        .starts_with("ambiguous:"));
    let selected = scoped(&nodes, "dialog:One").unwrap();
    assert_eq!(
        unique(&selected, "button", Some("Save"))
            .unwrap()
            .backend_dom_node_id,
        Some(7)
    );
    assert!(unique(&selected, "button", Some("Missing"))
        .unwrap_err()
        .starts_with("not_found:"));
}

#[test]
fn scoped_names_follow_snapshot_whitespace_normalization() {
    let nodes: Vec<AxNode> = serde_json::from_value(json!([
        {"nodeId":"a","role":{"value":"button"},"name":{"value":"Save\n  draft"},"backendDOMNodeId":7}
    ])).unwrap();
    assert_eq!(
        unique(&nodes, "button", Some("Save draft"))
            .unwrap()
            .backend_dom_node_id,
        Some(7)
    );
}
