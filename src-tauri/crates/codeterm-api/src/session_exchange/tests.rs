use super::capture_snapshot;
use super::import_target_allowed;
use codeterm_service_core::session::store::SessionSnapshot;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::json;

#[test]
fn populated_snapshot_exports_unavailable_leaf_and_remaining_histories() {
    let leaf = |id: &str, extra: serde_json::Value| {
        let mut value = json!({"type":"Leaf","tab_id":id,"cwd":"/private/project",
            "agent_type":"fixture-provider","agent_session_id":format!("session-{id}")});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };
    let snapshot: SessionSnapshot = serde_json::from_value(json!({
        "version":"1","timestamp":1,"active_workspace_id":"workspace",
        "workspaces":[{"id":"workspace","name":"private desk","active_tab_id":"group",
            "tabs":[{"id":"group","title":"private group","split_tree":{
                "type":"Split","orientation":"Horizontal","ratio":0.5,"children":[
                    leaf("unavailable", json!({"chatbackend_plugin_id":"fixture-provider",
                        "chatbackend_session_id":"session-unavailable"})),
                    leaf("readable", json!({})),
                    leaf("empty", json!({})),
                    leaf("shell", json!({"agent_type":null,"agent_session_id":null}))
                ]}}]}]
    }))
    .unwrap();
    let mut visited = Vec::new();
    let (safe, histories, measurements) = capture_snapshot(&snapshot, |node| {
        let (_, session) = node.agent_session().unwrap();
        visited.push(session.to_owned());
        match session {
            "session-unavailable" => Err("private transcript path".into()),
            "session-readable" => Ok(vec![ChatMsg {
                id: "message-readable".into(),
                kind: "assistant".into(),
                content: "private message".into(),
                tool_flow_id: Some("turn-readable".into()),
                ..Default::default()
            }]),
            "session-empty" => Ok(Vec::new()),
            _ => panic!("Non-agent leaf must not request a transcript"),
        }
    })
    .unwrap();
    let restored: SessionSnapshot = serde_json::from_value(safe.clone()).unwrap();
    assert_eq!(restored.workspaces[0].tabs.len(), 1);
    assert_eq!(
        safe["workspaces"][0]["tabs"][0]["split_tree"]["children"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        visited,
        ["session-unavailable", "session-readable", "session-empty"]
    );
    assert_eq!(histories.as_array().unwrap().len(), 4);
    assert_eq!(histories[0]["tab_id"], "unavailable");
    assert_eq!(histories[0]["transcript"]["status"], "unavailable");
    assert_eq!(histories[0]["transcript"]["backing_kind"], "chat_backend");
    assert_eq!(histories[0]["transcript"]["provider"], "fixture-provider");
    assert_eq!(histories[0]["messages"], json!([]));
    assert_eq!(histories[1]["transcript"]["status"], "captured");
    assert_eq!(histories[1]["messages"][0]["id"], "message-readable");
    assert_eq!(
        histories[1]["messages"][0]["content"],
        "x".repeat("private message".len())
    );
    assert_eq!(histories[2]["transcript"]["status"], "captured");
    assert_eq!(histories[2]["messages"], json!([]));
    assert_eq!(histories[3]["transcript"]["status"], "not_applicable");
    assert_eq!(histories[3]["transcript"]["backing_kind"], "pty");
    assert_eq!(measurements.len(), 4);
    assert_eq!(measurements[1]["history_items"], 1);
    assert_eq!(
        measurements[1]["history_bytes"],
        serde_json::to_vec(&histories[1]["messages"]).unwrap().len()
    );
    assert!(!safe.to_string().contains("private"));
    assert!(!histories.to_string().contains("private"));
}

#[test]
fn fixture_import_cannot_reach_production_or_ordinary_dev() {
    assert!(!import_target_allowed(
        false,
        7686,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(
        true,
        7685,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(true, 7686, ""));
    assert!(!import_target_allowed(true, 7686, "-another-team"));
    assert!(import_target_allowed(
        true,
        17686,
        "-upgrade-restore-fixture"
    ));
}
