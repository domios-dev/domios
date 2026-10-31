use super::*;
use crate::agent_runtime::*;
use serde_json::{json, Value};

fn context() -> RuntimeContext {
    RuntimeContext {
        restricted_cli: None,
        host_owned: Default::default(),
        epoch: 1,
        tab_id: "tab".into(),
        binding_nonce: "nonce".into(),
        thread_id: Some("root".into()),
        cwd: "/work".into(),
        host_version: "1.14.9".into(),
        thread_parents: vec![RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "root".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn descendant_authority_is_read_only_and_requires_current_ancestry() {
    let ctx = context();
    let read = RuntimeOperation::ReadHistory {
        thread_id: "child".into(),
        cursor: None,
        order: Some(HistoryOrder::Desc),
    };
    assert!(validate_runtime_operation(&ctx, &read).is_ok());
    assert!(!runtime_descendant(&ctx, "foreign"));
    let mut stale = ctx.clone();
    stale.thread_parents.clear();
    assert!(validate_runtime_operation(&stale, &read).is_err());
    for operation in [
        RuntimeOperation::ResumeThread {
            thread_id: "child".into(),
            instructions: "".into(),
            settings: Default::default(),
        },
        RuntimeOperation::SubmitTurn {
            thread_id: "child".into(),
            active_turn_id: None,
            send_id: "send".into(),
            input: TurnInput {
                text: "hello".into(),
                attachments: vec![],
            },
            settings: Default::default(),
        },
        RuntimeOperation::Control {
            thread_id: Some("child".into()),
            action: "compact".into(),
            value: Value::Null,
        },
    ] {
        assert!(validate_runtime_operation(&ctx, &operation).is_err());
    }
    stale.thread_parents.push(RuntimeThreadParent {
        thread_id: "child".into(),
        parent_thread_id: "child".into(),
    });
    assert!(!runtime_descendant(&stale, "child"));
}

#[test]
fn captured_spawn_metadata_requires_provider_parent_proof() {
    let root = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut directories: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|dir| dir.join("tests/runtime-manifest.json").is_file())
        .collect();
    directories.sort();
    let mut cases = Vec::new();
    for name in ["subagent", "subagent_v2"] {
        let paths: Vec<_> = directories
            .iter()
            .map(|dir| dir.join(format!("tests/fixtures/runtime/captured/{name}.json")))
            .filter(|path| path.is_file())
            .collect();
        assert!(!paths.is_empty(), "missing recorded {name} scenario");
        cases.extend(paths.into_iter().map(|path| (name, path)));
    }
    for (name, path) in cases {
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let root = fixture["context"]["threadId"].as_str().unwrap();
        let mut count = 0;
        for event in fixture["expected"]["events"].as_array().unwrap() {
            if event["kind"] != "chatItem" {
                continue;
            }
            let mut item: ChatMsg = serde_json::from_value(event["item"].clone()).unwrap();
            if item.tool_kind.as_deref() != Some("agent") {
                continue;
            }
            let child = runtime_child(root, &item).unwrap();
            assert_eq!(child.parent_thread_id, root);
            assert_ne!(child.thread_id, root);
            assert!(child.ordinal > 0);
            if name == "subagent" {
                assert!(child.prompt.is_some());
                assert!(child.model.is_some());
                assert!(child.effort.is_some());
            }
            item.tool_input
                .as_mut()
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("agent_parent_thread_id");
            assert!(runtime_child(root, &item).is_none());
            count += 1;
        }
        assert!(count > 0);
    }
}

#[test]
fn descendant_history_refuses_notifications_and_response_side_effects() {
    let ctx = context();
    let read = RuntimeOperation::ReadHistory {
        thread_id: "child".into(),
        cursor: None,
        order: None,
    };
    let capability: AgentRuntimeCapability = serde_json::from_value(json!({"runtime":"stdio-jsonrpc", "command":{"binary":"fixture","args":[]},"requestMethods":{"readHistory":["history/read"]},"notificationMethods":[]})).unwrap();
    let call = RpcCall {
        method: "history/read".into(),
        params: json!({"thread":"child"}),
        target: Some(RuntimeCallTarget {
            thread_id: "child".into(),
            turn_id: None,
        }),
    };
    let mut plan = RuntimePlan {
        action: RuntimeAction::Request { call: call.clone() },
        state: Value::Null,
    };
    assert!(validate_runtime_plan(&capability, &ctx, &read, &plan).is_ok());
    let mut foreign = call.clone();
    foreign.target.as_mut().unwrap().thread_id = "foreign".into();
    assert!(validate_runtime_plan(
        &capability,
        &ctx,
        &read,
        &RuntimePlan {
            action: RuntimeAction::Request { call: foreign },
            state: Value::Null
        }
    )
    .is_err());
    plan.action = RuntimeAction::Notification {
        notification: call.clone(),
    };
    assert!(validate_runtime_plan(&capability, &ctx, &read, &plan).is_err());
    let step = RuntimeStep::Complete {
        result: RuntimeResult::HistoryPage {
            thread_id: "child".into(),
            items: vec![],
            open_items: vec![],
            questions: None,
            next_cursor: None,
            order: None,
        },
        notification: Some(call),
        events: None,
    };
    assert!(validate_runtime_step(
        &capability,
        &ctx,
        &read,
        &RpcReply::Success { value: Value::Null },
        &step
    )
    .is_err());
}

#[test]
fn metadata_cannot_attach_a_receiver_to_an_unrecorded_parent() {
    let ctx = context();
    let item = ChatMsg {
        id: "spawn".into(),
        tool_kind: Some("agent".into()),
        tool_name: Some("agent_activity".into()),
        tool_input: Some(
            json!({"agent_id":"receiver","agent_parent_thread_id":"foreign","agent_ordinal":2}),
        ),
        ..Default::default()
    };
    let reduction = RuntimeReduction {
        state: Value::Null,
        events: vec![AgentRuntimeEvent::ChatItem {
            thread_id: "root".into(),
            item,
        }],
    };
    assert!(validate_runtime_reduction(&ctx, &reduction).is_err());
}
