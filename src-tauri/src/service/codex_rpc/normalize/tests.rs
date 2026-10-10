use super::*;
use serde_json::json;

#[test]
fn codex_rpc_normalize_split_deltas_preserve_exact_line_structure() {
    for text in [
        "first  \nsecond\n\nthird\\\nfourth",
        "first  \r\nsecond\r\n\r\nthird\\\r\nfourth",
    ] {
        for split in 0..=text.len() {
            let mut normalizer = Normalizer::default();
            let mut accumulated = String::new();
            for delta in [&text[..split], &text[split..]] {
                accumulated.push_str(delta);
                let message = normalizer
                    .apply(&Frame::Notification {
                        method: "item/agentMessage/delta".into(),
                        params: json!({"threadId":"bound","itemId":"verse","delta":delta}),
                    })
                    .unwrap();
                assert_eq!(message.content, accumulated);
                assert_eq!(normalizer.snapshot("bound")[0].content, accumulated);
            }
            assert_eq!(normalizer.snapshot("bound")[0].content, text);
            let completed = normalizer.apply(&Frame::Notification {
                method: "item/completed".into(),
                params: json!({"threadId":"bound","item":{"id":"verse","type":"agentMessage","text":text}}),
            }).unwrap();
            assert_eq!(completed.content, text);
            assert_eq!(normalizer.snapshot("bound")[0].content, text);
        }
    }
}

#[test]
fn codex_rpc_control_restored_items_keep_order_and_do_not_replay_old_deltas() {
    let messages = vec![ChatMsg {
        id: "old".into(),
        seq: 9,
        kind: "assistant".into(),
        content: "snapshot".into(),
        ..Default::default()
    }];
    let mut normalizer = Normalizer::from_history("bound", messages);
    assert!(normalizer
        .apply(&Frame::Notification {
            method: "item/agentMessage/delta".into(),
            params: json!({"threadId":"bound","itemId":"old","delta":"snapshot"})
        })
        .is_none());
    let completed = normalizer.apply(&Frame::Notification { method: "item/completed".into(), params: json!({"threadId":"bound","item":{"id":"old","type":"agentMessage","text":"whole"}}) }).unwrap();
    assert_eq!(completed.seq, 9);
    assert_eq!(completed.content, "whole");
    let next = normalizer
        .apply(&Frame::Notification {
            method: "item/agentMessage/delta".into(),
            params: json!({"threadId":"bound","itemId":"next","delta":"new"}),
        })
        .unwrap();
    assert_eq!(next.seq, 10);
    assert_eq!(next.content, "new");
}

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
    assert_eq!(edit.file_diffs.as_ref().unwrap()[0]["additions"], 1);
    assert_eq!(
        edit.file_diffs.as_ref().unwrap()[0]["hunks"][0]["lines"][0],
        "+codex app-server approval fixture."
    );
    assert!(messages
        .values()
        .any(|message| message.kind == "thinking" && !message.content.is_empty()));
}

#[test]
fn codex_rpc_mixed_turn_live_and_history_match_chat_wire_contract() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/mixed-turn.json")).unwrap();
    let items = fixture["items"].as_array().unwrap();
    let mut live = Normalizer::default();
    let mut history = Normalizer::default();
    for item in items {
        let mut started = item.clone();
        if item["type"] == "commandExecution" {
            started["status"] = json!("inProgress");
            started["aggregatedOutput"] = Value::Null;
            started["exitCode"] = Value::Null;
            started["durationMs"] = Value::Null;
        }
        live.apply(&Frame::Notification {
            method: "item/started".into(),
            params: json!({"threadId":"thread","turnId":"turn","item":started}),
        })
        .unwrap();
        if item["type"] == "commandExecution" {
            let delta = live.apply(&Frame::Notification { method:"item/commandExecution/outputDelta".into(),
                params: json!({"threadId":"thread","turnId":"turn","itemId":item["id"],"delta":"blueprint one\n"}) }).unwrap();
            assert_eq!(delta.tool_result.as_deref(), Some("blueprint one\n"));
            assert_eq!(delta.tool_kind.as_deref(), Some("command"));
            assert_eq!(delta.tool_error, None);
        }
        let entry = json!({"item":item,"turnId":"turn"});
        super::super::transcript::normalize_entry(&mut live, "thread", &entry).unwrap();
        super::super::transcript::normalize_entry(&mut history, "thread", &entry).unwrap();
    }
    assert_eq!(live.snapshot("thread"), history.snapshot("thread"));
    assert_eq!(
        serde_json::to_value(live.snapshot("thread")).unwrap(),
        fixture["messages"]
    );
}

#[test]
fn codex_rpc_all_schema_items_have_live_and_history_models() {
    let schema: Value =
        serde_json::from_str(include_str!("../schema/v2/ItemStartedNotification.json")).unwrap();
    for variant in schema["definitions"]["ThreadItem"]["oneOf"]
        .as_array()
        .unwrap()
    {
        let kind = variant["properties"]["type"]["enum"][0].as_str().unwrap();
        let item = json!({"id":kind,"type":kind,"content":[],"text":"body","summary":["summary"],
            "changes":[],"arguments":{"key":"value"},"tool":"skill","server":"codeterm",
            "query":"query","path":"image.png","review":"review","fragments":[{"text":"hook"}]});
        let frame = |method: &str| Frame::Notification {
            method: method.into(),
            params: json!({"threadId":"thread","turnId":"turn","item":item}),
        };
        let mut live = Normalizer::default();
        live.apply(&frame("item/started"))
            .unwrap_or_else(|| panic!("missing started {kind}"));
        let completed = live
            .apply(&frame("item/completed"))
            .unwrap_or_else(|| panic!("missing completed {kind}"));
        let replay = super::super::transcript::normalize_entry(
            &mut Normalizer::default(),
            "thread",
            &json!({"item":item,"turnId":"turn"}),
        )
        .unwrap();
        assert_eq!(completed, replay, "{kind}");
        if completed.kind == "tool_call" {
            assert!(
                matches!(
                    completed.tool_kind.as_deref(),
                    Some("command" | "edit" | "generic" | "web" | "image" | "agent")
                ),
                "{kind}"
            );
            assert!(completed.collapsed);
        }
    }
}

#[test]
fn codex_rpc_mcp_results_errors_progress_and_arguments_keep_exact_content() {
    let mut normalizer = Normalizer::default();
    let event = |method: &str, item: Value| Frame::Notification {
        method: method.into(),
        params: json!({"threadId":"t","turnId":"turn","item":item}),
    };
    let item = json!({"type":"mcpToolCall","id":"mcp","server":"codeterm","tool":"skill",
        "arguments":{"name":"style"},"status":"inProgress","result":null,"error":null});
    let started = normalizer
        .apply(&event("item/started", item.clone()))
        .unwrap();
    assert_eq!(started.tool_kind.as_deref(), Some("generic"));
    assert_eq!(started.tool_title.as_deref(), Some("codeterm · skill"));
    assert_eq!(started.tool_summary.as_deref(), Some("name: style"));
    assert_eq!(started.tool_error, None);
    let progress = normalizer
        .apply(&Frame::Notification {
            method: "item/mcpToolCall/progress".into(),
            params: json!({"threadId":"t","itemId":"mcp","message":"loading"}),
        })
        .unwrap();
    assert_eq!(progress.tool_detail.as_deref(), Some("loading"));
    let mut completed = item;
    completed["status"] = json!("completed");
    completed["result"] =
        json!({"content":[{"type":"text","text":"skill body"}],"structuredContent":{"count":2}});
    let done = normalizer
        .apply(&event("item/completed", completed.clone()))
        .unwrap();
    assert_eq!(done.tool_error, Some(false));
    assert_eq!(
        serde_json::from_str::<Value>(done.tool_result.as_deref().unwrap()).unwrap(),
        completed["result"]
    );
    completed["status"] = json!("failed");
    completed["error"] = json!({"message":"unavailable"});
    let failed = normalizer
        .apply(&event("item/completed", completed))
        .unwrap();
    assert_eq!(failed.tool_error, Some(true));
    assert!(failed.tool_result.unwrap().contains("unavailable"));
}

#[test]
fn codex_rpc_user_images_compaction_plans_and_turn_errors_are_preserved() {
    let mut normalizer = Normalizer::default();
    let user = super::super::transcript::normalize_entry(&mut normalizer,"t",&json!({"id":"u","type":"userMessage",
        "content":[{"type":"text","text":"look"},{"type":"image","url":"data:image/jpeg;base64,YQ=="},{"type":"localImage","path":"/shot.webp"}]})).unwrap();
    assert_eq!(user.content, "look");
    let images = user.attachments.unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].mime_type, "image/jpeg");
    assert_eq!(images[0].data.as_deref(), Some("YQ=="));
    assert_eq!(images[1].path.as_deref(), Some("/shot.webp"));
    let compact = super::super::transcript::normalize_entry(
        &mut normalizer,
        "t",
        &json!({"id":"c","type":"contextCompaction"}),
    )
    .unwrap();
    assert_eq!(
        compact.timeline_marker,
        Some(crate::agent::chat_types::ChatTimelineMarker::ContextCompaction)
    );
    let plan = normalizer
        .apply(&Frame::Notification {
            method: "turn/plan/updated".into(),
            params: json!({"threadId":"t","turnId":"turn",
        "explanation":"inspect","plan":[{"step":"read","status":"inProgress"}]}),
        })
        .unwrap();
    assert_eq!(plan.tool_name.as_deref(), Some("update_plan"));
    assert!(plan.tool_result.unwrap().contains("inProgress"));
    let errors = normalizer.apply_all(&Frame::Notification {
        method: "turn/completed".into(),
        params: json!({"threadId":"t",
        "turn":{"id":"turn","status":"failed","error":{"message":"failed request"}}}),
    });
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].kind, "system");
    assert_eq!(errors[0].content, "failed request");
    assert_eq!(errors[1].kind, "tool_flow");
    assert_eq!(errors[1].content, "end");
}

#[test]
fn codex_rpc_rich_tools_diffs_and_agent_receivers_preserve_details() {
    let mut normalizer = Normalizer::default();
    let entry = |item| json!({"turnId":"turn","item":item});
    let web = super::super::transcript::normalize_entry(&mut normalizer,"t",&entry(json!({
        "id":"web","type":"webSearch","query":"rust","action":{"type":"search"},"results":[{"url":"https://example.com"}]}))).unwrap();
    assert_eq!(web.tool_kind.as_deref(), Some("web"));
    assert_eq!(web.tool_rich_result.unwrap()["resultCount"], 1);
    let image = super::super::transcript::normalize_entry(&mut normalizer,"t",&entry(json!({
        "id":"image","type":"imageGeneration","savedPath":"/generated.png","result":"YQ==","status":"completed"}))).unwrap();
    assert_eq!(
        image.tool_result.as_deref(),
        Some("data:image/png;base64,YQ==")
    );
    assert_eq!(image.tool_rich_result.unwrap()["path"], "/generated.png");
    let edit = super::super::transcript::normalize_entry(&mut normalizer,"t",&entry(json!({
        "id":"patch","type":"fileChange","status":"completed","changes":[
            {"path":"old.rs","kind":{"type":"update","move_path":"new.rs"},"diff":"@@ -2,1 +2,2 @@\n-old\n+new\n+extra\n"},
            {"path":"gone.rs","kind":{"type":"delete"},"diff":"removed\n"},
            {"path":"image.png","kind":{"type":"add"},"diff":"GIT binary patch\nencoded\n"}]}))).unwrap();
    let diffs = edit.file_diffs.unwrap();
    assert_eq!(diffs[0]["movePath"], "new.rs");
    assert_eq!(diffs[0]["kind"], "rename");
    assert_eq!(diffs[0]["additions"], 2);
    assert_eq!(diffs[0]["deletions"], 1);
    assert_eq!(diffs[0]["hunks"][0]["oldStart"], 2);
    assert_eq!(diffs[1]["hunks"][0]["lines"][0], "-removed");
    assert_eq!(diffs[1]["kind"], "delete");
    assert!(diffs[1].get("movePath").is_none());
    assert_eq!(diffs[2]["binary"], true);
    assert_eq!(diffs[2]["hunks"], json!([]));
    assert_eq!(diffs[2]["additions"], 0);
    let agents = normalizer.apply_all(&Frame::Notification { method:"item/completed".into(),params:json!({"threadId":"t","turnId":"turn",
        "item":{"id":"collab","type":"collabAgentToolCall","receiverThreadIds":["a","b"],"agentsStates":{"a":{"status":"running"},"b":{"status":"errored","message":"failed"}}}}) });
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].tool_kind.as_deref(), Some("agent"));
    assert_eq!(agents[0].tool_input.as_ref().unwrap()["agent_id"], "a");
    assert_eq!(
        agents[1].tool_input.as_ref().unwrap()["agent_state"],
        "failed"
    );
    let goal = normalizer
        .apply(&Frame::Notification {
            method: "thread/goal/updated".into(),
            params: json!({"threadId":"t","goal":{"objective":"inspect"}}),
        })
        .unwrap();
    assert_eq!(
        goal.session_intent.unwrap().goal.as_deref(),
        Some("inspect")
    );
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

#[test]
fn codex_rpc_command_batch_identity_matches_live_and_history() {
    let mut live = Normalizer::default();
    let mut history = Normalizer::default();
    for id in ["one", "two", "three"] {
        let item = json!({"id":id,"type":"commandExecution","command":"pwsh -Command 'Get-Location'","commandActions":[{"type":"unknown","command":"Get-Location"}],"status":"completed","exitCode":0,"aggregatedOutput":id});
        let mut started = item.clone();
        started["status"] = json!("inProgress");
        started["exitCode"] = Value::Null;
        live.apply(&Frame::Notification {
            method: "item/started".into(),
            params: json!({"threadId":"thread","turnId":"batch","item":started}),
        })
        .unwrap();
        let entry = json!({"turnId":"batch","item":item});
        super::super::transcript::normalize_entry(&mut live, "thread", &entry).unwrap();
        super::super::transcript::normalize_entry(&mut history, "thread", &entry).unwrap();
    }
    assert_eq!(live.snapshot("thread"), history.snapshot("thread"));
    for message in live.snapshot("thread") {
        let input = message.tool_input.unwrap();
        assert_eq!(input["commandBatchId"], "batch");
        assert_eq!(input["commandActions"][0]["command"], "Get-Location");
        assert_eq!(
            message.tool_detail.as_deref(),
            Some("pwsh -Command 'Get-Location'")
        );
        assert_eq!(message.tool_result.as_deref(), Some(message.id.as_str()));
    }
}
