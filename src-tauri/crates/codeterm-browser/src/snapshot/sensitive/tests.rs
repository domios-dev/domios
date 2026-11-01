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
