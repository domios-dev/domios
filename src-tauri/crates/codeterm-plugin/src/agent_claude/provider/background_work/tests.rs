use super::*;

#[test]
fn background_boundaries_are_structural() {
    for row in [
        serde_json::json!({"type":"summary"}),
        serde_json::json!({"type":"user","isCompactSummary":true}),
        serde_json::json!({"type":"system","subtype":"init"}),
        serde_json::json!({"type":"system","subtype":"resume"}),
    ] {
        assert_eq!(parse(&row)[0].state, BackgroundWorkState::SessionBoundary);
    }
    assert!(parse(&serde_json::json!({"type":"user","subtype":"compact_boundary"})).is_empty());
}
