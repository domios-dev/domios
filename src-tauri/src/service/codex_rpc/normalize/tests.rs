use super::*;
use serde_json::json;

fn replay(data: &str) -> HashMap<String, ChatMsg> {
    let mut normalizer = Normalizer::default();
    let mut messages: HashMap<String, ChatMsg> = HashMap::new();
    for line in data.lines() {
        let frame = super::super::protocol::decode(line).unwrap();
        if let Some(message) = normalizer.apply(&frame) {
            if let Some(previous) = messages.get(&message.id) {
                assert_eq!(message.seq, previous.seq);
            }
            messages.insert(message.id.clone(), message);
        }
    }
    messages
}

#[test]
fn codex_rpc_normalize_recorded_simple_keeps_app_server_item_id() {
    let messages = replay(include_str!("fixtures/simple.ndjson"));
    let message = &messages["msg_08fda0295ef7a774016ab5bd70c40887d080d27158f71dce50"];
    assert_eq!(message.kind, "assistant");
    assert_eq!(message.content, "fixture simple ok");
    assert_eq!(
        messages
            .values()
            .filter(|message| message.kind == "assistant")
            .count(),
        1
    );
}

#[test]
fn codex_rpc_normalize_recorded_command_preserves_outcome() {
    let messages = replay(include_str!("fixtures/tool_call_workspace_never.ndjson"));
    let message = &messages["exec-0aedac8d-7d1e-4a69-8d3e-14fa41449e6e"];
    assert_eq!(message.kind, "tool_call");
    assert_eq!(message.tool_exit_code, Some(0));
    assert_eq!(message.tool_duration_ms, Some(0));
    assert_eq!(message.tool_error, Some(false));
    assert_eq!(message.tool_name.as_deref(), Some("commandExecution"));
}

#[test]
fn codex_rpc_normalize_recorded_file_changes_and_reasoning() {
    let messages = replay(include_str!(
        "fixtures/tool_call_restricted_granular.ndjson"
    ));
    let edit = &messages["exec-114e8eef-018a-444d-8542-aa0232ff1f38"];
    assert_eq!(edit.tool_name.as_deref(), Some("fileChange"));
    assert_eq!(edit.file_diffs.as_ref().unwrap()[0]["kind"]["type"], "add");
    assert_eq!(
        edit.file_diffs.as_ref().unwrap()[0]["diff"],
        "codex app-server approval fixture.\n"
    );
    assert!(messages
        .values()
        .any(|message| message.kind == "thinking" && !message.content.is_empty()));
}

#[test]
fn codex_rpc_normalize_deltas_accumulate_without_changing_identity() {
    let mut normalizer = Normalizer::default();
    for (method, kind) in [
        ("item/agentMessage/delta", "assistant"),
        ("item/reasoning/textDelta", "thinking"),
        ("item/reasoning/summaryTextDelta", "thinking"),
        ("item/commandExecution/outputDelta", "tool_call"),
        ("item/fileChange/outputDelta", "tool_call"),
    ] {
        let id = method.to_owned();
        let event = |delta| Frame::Notification {
            method: method.into(),
            params: json!({"threadId":"t","itemId":id,"delta":delta}),
        };
        let first = normalizer.apply(&event("left")).unwrap();
        let second = normalizer.apply(&event("right")).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.seq, second.seq);
        assert_eq!(second.kind, kind);
        assert_eq!(
            if kind == "tool_call" {
                second.tool_result.unwrap()
            } else {
                second.content
            },
            "leftright"
        );
    }
}

#[test]
fn codex_rpc_normalize_ignores_unknown_events_and_isolates_threads() {
    let mut normalizer = Normalizer::default();
    let event = |thread, delta| Frame::Notification {
        method: "item/agentMessage/delta".into(),
        params: json!({"threadId":thread,"itemId":"same","delta":delta}),
    };
    normalizer.apply(&event("a", "a"));
    assert_eq!(normalizer.apply(&event("b", "b")).unwrap().content, "b");
    assert_eq!(normalizer.apply(&event("a", "c")).unwrap().content, "ac");
    assert!(normalizer
        .apply(&Frame::Notification {
            method: "future".into(),
            params: json!({"threadId":"a","itemId":"same"})
        })
        .is_none());
}
