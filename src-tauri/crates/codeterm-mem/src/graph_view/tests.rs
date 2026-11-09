use super::*;
use serde_json::json;

#[test]
fn graph_view_preserves_selected_fields_and_explicit_nulls() {
    let node = MemNode {
        id: "fact_one".into(),
        kind: "fact".into(),
        title: "One".into(),
        summary: None,
        area: None,
        importance: 0.5,
        tags: vec![],
        has_vector: false,
        has_body: false,
        author: None,
    };
    assert_eq!(
        serde_json::to_value(node).unwrap(),
        json!({
            "id": "fact_one", "kind": "fact", "title": "One", "summary": null,
            "area": null, "importance": 0.5, "tags": [], "has_vector": false,
            "has_body": false, "author": null,
        })
    );
    let edge = crate::store::GraphEdge {
        from: "fact_one".into(),
        to: "fact_two".into(),
        rel: "related".into(),
        confidence: 1.0,
        valid_from: "2026-01-01".into(),
        valid_to: None,
    };
    assert_eq!(
        serde_json::to_value(edge).unwrap(),
        json!({
            "from": "fact_one", "to": "fact_two", "rel": "related", "confidence": 1.0,
            "valid_from": "2026-01-01", "valid_to": null,
        })
    );
}
