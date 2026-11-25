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

#[test]
fn task_notification_origin_and_stopped_statuses_end_matching_tasks() {
    let user_notice = serde_json::json!({
        "type": "user",
        "origin": { "kind": "task-notification", "producer": "session-task" },
        "message": {
            "role": "user",
            "content": "<task-notification>\n<task-id>shell-a</task-id>\n<status>completed</status>\n</task-notification>"
        }
    });
    assert_eq!(parse(&user_notice).len(), 1);
    assert_eq!(parse(&user_notice)[0].id, "shell-a");
    assert_eq!(parse(&user_notice)[0].state, BackgroundWorkState::Completed);

    let multi_stopped = serde_json::json!({
        "type": "user",
        "origin": { "kind": "task-notification" },
        "message": {
            "role": "user",
            "content": "<task-notification>\n<task-id>orphan-1</task-id>\n<task-id>orphan-2</task-id>\n<task-id>__orphan_summary__:shell</task-id>\n<status>stopped</status>\n</task-notification>"
        }
    });
    let stopped = parse(&multi_stopped);
    assert_eq!(
        stopped
            .iter()
            .map(|o| (o.id.as_str(), o.state))
            .collect::<Vec<_>>(),
        [
            ("orphan-1", BackgroundWorkState::Killed),
            ("orphan-2", BackgroundWorkState::Killed),
        ]
    );

    let human = serde_json::json!({
        "type": "user",
        "origin": { "kind": "human" },
        "message": {
            "role": "user",
            "content": "<task-notification><task-id>shell-a</task-id><status>completed</status></task-notification>"
        }
    });
    assert!(parse(&human).is_empty());

    let plain_user = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": "<task-notification><task-id>shell-a</task-id><status>completed</status></task-notification>"
        }
    });
    assert!(parse(&plain_user).is_empty());
}

#[test]
fn tool_use_and_tool_result_label_background_shell_from_structured_records() {
    let assistant = serde_json::json!({
        "type": "assistant",
        "sessionId": "opaque-own",
        "message": {
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "opaque-call-a",
                    "name": "Bash",
                    "input": { "command": "opaque-cmd-a 900", "run_in_background": true }
                },
                {
                    "type": "tool_use",
                    "id": "opaque-call-b",
                    "name": "Bash",
                    "input": { "command": "opaque-runner -e tick()" }
                }
            ]
        }
    });
    let launch_a = serde_json::json!({
        "type": "user",
        "sessionId": "opaque-own",
        "toolUseResult": { "backgroundTaskId": "opaque-task-a" },
        "message": {
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": "opaque-call-a", "content": "ok", "is_error": false }]
        }
    });
    let launch_b = serde_json::json!({
        "type": "user",
        "sessionId": "opaque-own",
        "toolUseResult": { "backgroundTaskId": "opaque-task-b" },
        "message": {
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": "opaque-call-b", "content": "ok", "is_error": false }]
        }
    });
    assert!(parse(&assistant).is_empty());
    let calls: std::collections::HashMap<String, String> = parse_tool_calls(&assistant)
        .into_iter()
        .map(|call| (call.id, call.label))
        .collect();
    assert_eq!(
        calls.get("opaque-call-a").map(String::as_str),
        Some("opaque-cmd-a 900")
    );
    assert_eq!(
        calls.get("opaque-call-b").map(String::as_str),
        Some("opaque-runner -e tick()")
    );

    let mut roster = codeterm_types::agent::background_work::BackgroundRoster::default();
    for row in [&launch_a, &launch_b] {
        for observation in parse(row) {
            let label = observation
                .tool_use_id
                .as_deref()
                .and_then(|id| calls.get(id))
                .map(String::as_str)
                .unwrap_or("");
            roster.observe(&observation, label);
        }
    }
    assert_eq!(
        roster.running.get("opaque-task-a").map(String::as_str),
        Some("opaque-cmd-a 900")
    );
    assert_eq!(
        roster.running.get("opaque-task-b").map(String::as_str),
        Some("opaque-runner -e tick()")
    );
    assert_eq!(roster.running.len(), 2);
}
