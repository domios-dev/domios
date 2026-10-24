use super::*;

#[test]
fn task_stop_ends_background_work_from_opaque_fixture() {
    let rows = [
        serde_json::json!({"type":"user", "toolUseResult":{"backgroundTaskId":"opaque-stopped"}}),
        serde_json::json!({"type":"user", "toolUseResult":{
            "task_id":"opaque-stopped", "task_type":"local_bash", "command":"opaque-c",
            "message":"opaque-b"
        }, "message":{"content":[{"type":"tool_result", "content":"opaque-d"}]}}),
    ];
    let mut roster = codeterm_types::agent::background_work::BackgroundRoster::default();
    for row in &rows[rows.len() - 2..] {
        let observations = parse(row);
        assert_eq!(observations.len(), 1);
        for observation in observations {
            roster.observe(&observation, "");
        }
    }
    assert!(roster.running.is_empty());
    assert!(roster.ended.contains_key("opaque-stopped"));
    let stop = rows.last().unwrap();
    assert_eq!(parse(stop)[0].state, BackgroundWorkState::Killed);
    for (field, value) in [
        ("task_type", serde_json::json!("opaque")),
        ("task_id", serde_json::json!("../foreign")),
        ("command", Value::Null),
    ] {
        let mut invalid = stop.clone();
        invalid["toolUseResult"][field] = value;
        assert!(parse(&invalid).is_empty());
    }
    let mut failed = stop.clone();
    failed["message"]["content"][0]["is_error"] = Value::Bool(true);
    assert!(parse(&failed).is_empty());
    let mut assistant = stop.clone();
    assistant["type"] = serde_json::json!("assistant");
    assert!(parse(&assistant).is_empty());
}

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
