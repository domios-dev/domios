use crate::agent_runtime::*;
use crate::js_runtime::{JsProvider, PluginManifest, PluginVm};
use crate::provider::AgentProvider;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "runtime_native.rs"]
mod native;
#[path = "runtime_replay.rs"]
mod replay;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenericFixture {
    context: RuntimeContext,
    operations: Vec<OperationCase>,
    reductions: Vec<ReductionCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationCase {
    operation: RuntimeOperation,
    context: Option<RuntimeContext>,
    reply: Option<RpcReply>,
    plan: RuntimePlan,
    step: Option<RuntimeStep>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReductionCase {
    context: RuntimeContext,
    state: Value,
    frames: Vec<RuntimeFrame>,
    expected: RuntimeReduction,
}

fn generic_paths() -> PathBuf {
    std::env::var_os("CODETERM_GENERIC_RUNTIME_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(crate::test_support::fixture_manifest_dir())
                .join("src/js_runtime/parity_test/fixtures/runtime")
        })
}
fn generic_provider(dir: &Path) -> JsProvider {
    generic_provider_with_history(dir, true)
}

fn generic_provider_with_history(dir: &Path, stored_history: bool) -> JsProvider {
    let kinds = [
        "backgroundProcesses",
        "initialize",
        "openThread",
        "forkThread",
        "resumeThread",
        "readHistory",
        "submitTurn",
        "interrupt",
        "answer",
        "answerQuestion",
        "control",
    ];
    let methods: serde_json::Map<String, Value> = kinds
        .iter()
        .map(|kind| {
            let methods = if *kind == "forkThread" {
                json!(["thread/turns/list", "thread/fork"])
            } else {
                json!([kind])
            };
            ((*kind).into(), methods)
        })
        .collect();
    let manifest=PluginManifest::from_json_str(&json!({"id":"echo-runtime","version":"1.0.0","displayName":"Fixture","capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{"storedHistory":stored_history,"serviceTiers":[{"id":"priority","name":"Fast","description":""}],"backgroundProcesses":true,"usage":true,"threadHistory":{"actions":["fork","revert"]},"resetCreditsMinVersion":"0.162.1","runtime":"stdio-jsonrpc","command":{"binary":"fixture-engine","args":[]},"requestMethods":methods,"notificationMethods":["item"],"login":{"methods":["browser","device","apiKey"],"terminalCommand":"fixture-engine login"}}},"permissions":{"subprocess":{"allow":["fixture-engine"]}}}).to_string()).unwrap();
    let source = std::fs::read_to_string(dir.join("plugin.js"))
        .expect("generic runtime plugin path must exist");
    JsProvider::new(
        manifest,
        Arc::new(PluginVm::from_source("echo-runtime", source).unwrap()),
    )
}

#[test]
fn runtime_stored_history_native_quickjs_parity_and_capability_gate() {
    let dir = generic_paths();
    let content = "quotes: \"\nbackslash: \\\r\nUnicode: 🦀 escaped surrogates: \\ud800 \\udfff";
    let records = json!({"session":"own-session", "messages":[
        {"id":"own-item", "type":"user", "content":content, "seq":0}
    ]})
    .to_string();
    for enabled in [false, true] {
        let js = generic_provider_with_history(&dir, enabled);
        let native = native::NativeRuntime(js.agent_runtime().unwrap());
        for session in ["own-session", "reference-session"] {
            assert_eq!(
                native.runtime_stored_history_available("/work/project", session),
                js.runtime_stored_history_available("/work/project", session)
            );
            assert_eq!(
                js.runtime_stored_history_available("/work/project", session),
                enabled && session != "reference-session"
            );
        }
        let a = native.runtime_stored_history("own-session", &records);
        let b = js.runtime_stored_history("own-session", &records);
        assert_eq!(a.is_ok(), enabled);
        assert_eq!(b.is_ok(), enabled);
        if enabled {
            let native = serde_json::to_value(a.unwrap()).unwrap();
            let js = serde_json::to_value(b.unwrap()).unwrap();
            assert_eq!(native, js);
            assert_eq!(
                js[0]["content"].as_str().unwrap().as_bytes(),
                content.as_bytes()
            );
        }
        assert!(native
            .runtime_stored_history("foreign-session", &records)
            .is_err());
        assert!(js
            .runtime_stored_history("foreign-session", &records)
            .is_err());
    }
}

#[tokio::test]
async fn capacity_retry_signal_native_quickjs_parity() {
    let js = generic_provider(&generic_paths());
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    let ctx = RuntimeContext {
        epoch: 1,
        tab_id: "tab".into(),
        binding_nonce: "nonce".into(),
        thread_id: Some("own".into()),
        cwd: "/fixture".into(),
        host_version: "1.15.2".into(),
        ..Default::default()
    };
    for (kind, at) in [
        (Some("transient"), None),
        (Some("rate_limited_until"), Some(100_123u64)),
        (None, None),
    ] {
        let metadata = json!({"retryable":false,"retryableKind":kind,"retryAtMs":at});
        let frames = [RuntimeFrame::Notification {
            method: "item".into(),
            params: json!({"id":"own-error","type":"system","content":"opaque", "toolFlowId":"turn", "systemSubtype":"api_error","toolInput":metadata}),
        }];
        let expected = native
            .runtime_reduce(&ctx, &Value::Null, &frames)
            .await
            .unwrap();
        let actual = js
            .runtime_reduce(&ctx, &Value::Null, &frames)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let AgentRuntimeEvent::ChatItem { item, .. } = &actual.events[0] else {
            panic!("expected item")
        };
        let signal: RuntimeErrorSignal =
            serde_json::from_value(item.tool_input.clone().unwrap()).unwrap();
        assert_eq!(
            signal.retryable_kind,
            match kind {
                Some("transient") => Some(RuntimeRetryableErrorKind::Transient),
                Some("rate_limited_until") => Some(RuntimeRetryableErrorKind::RateLimitedUntil),
                _ => None,
            }
        );
        assert_eq!(signal.retry_at_ms, at);
        assert!(!signal.retryable);
    }
    assert!(serde_json::from_value::<RuntimeErrorSignal>(
        json!({"retryable":false,"retryableKind":"unknown"})
    )
    .is_err());
}

#[tokio::test]
async fn runtime_native_quickjs_shared_fixture_parity() {
    assert!(serde_json::from_value::<RuntimeServiceTierSelection>(json!({})).is_err());
    assert_eq!(
        serde_json::from_value::<RuntimeSettings>(json!({}))
            .unwrap()
            .service_tier,
        None
    );
    assert_eq!(
        serde_json::from_value::<RuntimeSettings>(json!({"serviceTier":{"id":null}}))
            .unwrap()
            .service_tier
            .unwrap()
            .id,
        None
    );
    let dir = generic_paths();
    let fixture: GenericFixture =
        serde_json::from_str(&std::fs::read_to_string(dir.join("shared.json")).unwrap()).unwrap();
    let js = generic_provider(&dir);
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    assert_eq!(js.agent_runtime(), native.agent_runtime());
    assert!(js.agent_runtime().unwrap().usage);
    assert_eq!(js.agent_runtime().unwrap().service_tiers.len(), 1);
    assert_eq!(
        serde_json::to_value(native.describe()).unwrap()["agent_runtime"]["serviceTiers"][0]["id"],
        "priority"
    );
    assert!(js.agent_runtime().unwrap().background_processes);
    assert_eq!(
        js.agent_runtime()
            .unwrap()
            .reset_credits_min_version
            .as_deref(),
        Some("0.162.1")
    );
    assert_eq!(
        js.describe().settings_schema,
        native.describe().settings_schema
    );
    assert_eq!(js.describe().agent_runtime, native.describe().agent_runtime);
    for value in [None, Some("priority"), Some("ultrafast")] {
        let operation = RuntimeOperation::OpenThread {
            instructions: String::new(),
            settings: RuntimeSettings {
                service_tier: Some(RuntimeServiceTierSelection {
                    id: value.map(str::to_owned),
                }),
                ..Default::default()
            },
        };
        let native_plan = native
            .runtime_plan(&fixture.context, &operation)
            .await
            .unwrap();
        let js_plan = js.runtime_plan(&fixture.context, &operation).await.unwrap();
        assert_eq!(native_plan, js_plan);
        let reply = RpcReply::Success {
            value: json!({"threadId":"fixture-thread"}),
        };
        let native_reply = native
            .runtime_response(&fixture.context, &operation, &native_plan.state, &reply)
            .await
            .unwrap();
        let js_reply = js
            .runtime_response(&fixture.context, &operation, &js_plan.state, &reply)
            .await
            .unwrap();
        assert_eq!(native_reply, js_reply);
        let RuntimeStep::Complete {
            result: RuntimeResult::ThreadLoaded { settings, .. },
            ..
        } = js_reply
        else {
            panic!()
        };
        assert_eq!(
            settings.service_tier,
            Some(RuntimeServiceTierSelection {
                id: value.map(str::to_owned)
            })
        );
    }
    for case in &fixture.operations {
        let ctx = case.context.as_ref().unwrap_or(&fixture.context);
        let a = native.runtime_plan(ctx, &case.operation).await.unwrap();
        let b = js.runtime_plan(ctx, &case.operation).await.unwrap();
        assert_eq!(a, case.plan);
        assert_eq!(b, a);
        if let (Some(reply), Some(expected)) = (&case.reply, &case.step) {
            let a = native
                .runtime_response(ctx, &case.operation, &a.state, reply)
                .await
                .unwrap();
            let b = js
                .runtime_response(ctx, &case.operation, &b.state, reply)
                .await
                .unwrap();
            assert_eq!(&a, expected);
            assert_eq!(b, a);
        } else {
            assert!(matches!(
                case.operation,
                RuntimeOperation::AnswerQuestion { .. }
            ));
            assert!(matches!(a.action, RuntimeAction::QuestionAnswer { .. }));
        }
    }
    for case in &fixture.reductions {
        let a = native
            .runtime_reduce(&case.context, &case.state, &case.frames)
            .await
            .unwrap();
        let b = js
            .runtime_reduce(&case.context, &case.state, &case.frames)
            .await
            .unwrap();
        assert_eq!(a, case.expected);
        assert_eq!(b, a);
        let huge = json!("x".repeat(MAX_RUNTIME_STATE_BYTES));
        assert_eq!(
            native
                .runtime_reduce(&case.context, &huge, &case.frames)
                .await
                .unwrap_err()
                .code,
            js.runtime_reduce(&case.context, &huge, &case.frames)
                .await
                .unwrap_err()
                .code
        );
        let bad = vec![RuntimeFrame::Notification {
            method: "item".into(),
            params: json!({"id":"bad"}),
        }];
        assert_eq!(
            native
                .runtime_reduce(&case.context, &case.state, &bad)
                .await
                .unwrap_err()
                .code,
            js.runtime_reduce(&case.context, &case.state, &bad)
                .await
                .unwrap_err()
                .code
        );
    }
}

#[test]
fn runtime_streamed_history_validation_preserves_bounds_and_binding() {
    let dir = generic_paths();
    let fixture: GenericFixture =
        serde_json::from_str(&std::fs::read_to_string(dir.join("shared.json")).unwrap()).unwrap();
    let js = generic_provider(&dir);
    let cap = js.agent_runtime().unwrap();
    let case = fixture
        .operations
        .iter()
        .find(|case| {
            matches!(
                case.step,
                Some(RuntimeStep::Continue {
                    history: Some(_),
                    ..
                })
            )
        })
        .unwrap();
    let ctx = case.context.as_ref().unwrap_or(&fixture.context);
    let reply = case.reply.as_ref().unwrap();
    let step = case.step.as_ref().unwrap();
    validate_runtime_step(&cap, ctx, &case.operation, reply, step).unwrap();
    let mut oversized = step.clone();
    if let RuntimeStep::Continue { plan, .. } = &mut oversized {
        plan.state = json!({"items":"x".repeat(MAX_RUNTIME_STATE_BYTES)});
    }
    assert_eq!(
        validate_runtime_step(&cap, ctx, &case.operation, reply, &oversized)
            .unwrap_err()
            .code,
        RuntimeFailureCode::SizeLimit
    );
    let mut foreign = step.clone();
    if let RuntimeStep::Continue {
        history: Some(RuntimeResult::HistoryPage { thread_id, .. }),
        ..
    } = &mut foreign
    {
        *thread_id = "foreign".into();
    }
    assert_eq!(
        validate_runtime_step(&cap, ctx, &case.operation, reply, &foreign)
            .unwrap_err()
            .code,
        RuntimeFailureCode::BindingMismatch
    );
    assert_eq!(
        validate_runtime_step(&cap, ctx, &RuntimeOperation::Initialize, reply, step)
            .unwrap_err()
            .code,
        RuntimeFailureCode::BindingMismatch
    );
}

#[tokio::test]
async fn bundled_history_hook_adapts_large_pages_and_projects_twenty_mb_item() {
    let manifest = std::fs::read_dir(super::workspace_plugins_root())
        .unwrap()
        .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path().join("plugin.json")).ok())
        .filter_map(|source| serde_json::from_str::<Value>(&source).ok())
        .find(|manifest| {
            manifest["capabilities"]["agentRuntime"]["requestMethods"]["readHistory"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|method| method == "thread/items/list"))
        })
        .expect("bundled paged-history fixture provider");
    let js = super::load_js(manifest["id"].as_str().unwrap());
    let ctx: RuntimeContext = serde_json::from_value(json!({
        "epoch":1,"tabId":"own-tab","bindingNonce":"nonce","cwd":"/project",
        "hostVersion":"1.14.9","threadId":"own-thread","claimedSends":[],
        "settings":{},"providerSettings":{}
    }))
    .unwrap();
    let op = RuntimeOperation::ReadHistory {
        thread_id: "own-thread".into(),
        cursor: None,
        order: Some(HistoryOrder::Desc),
    };
    let first = js.runtime_plan(&ctx, &op).await.unwrap();
    let RuntimeStep::Continue { mut plan, .. } = js
        .runtime_response(
            &ctx,
            &op,
            &first.state,
            &RpcReply::Success {
                value: json!({"data":[{"id":"turn","status":"completed"}],"nextCursor":null}),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected items request")
    };
    let large = RpcReply::Success {
        value: json!({"data":[{"item":{"id":"huge","type":"agentMessage","text":"x".repeat(MAX_RUNTIME_VALUE_BYTES + 1)}}],"nextCursor":"older"}),
    };
    for limit in [8, 4, 2, 1] {
        let RuntimeStep::Continue { plan: smaller, .. } = js
            .runtime_response(&ctx, &op, &plan.state, &large)
            .await
            .unwrap()
        else {
            panic!("expected smaller request")
        };
        let RuntimeAction::Request { call } = &smaller.action else {
            panic!("expected request")
        };
        assert_eq!(call.params["limit"], limit);
        assert_eq!(call.params["cursor"], Value::Null);
        plan = smaller;
    }
    let single = RpcReply::Success {
        value: json!({"data":[{"item":{
        "id":"huge","type":"userMessage","content":vec![json!({"type":"text","text":"x".repeat(1024)});20480]
    }}],"nextCursor":"older"}),
    };
    let RuntimeStep::Complete {
        result: RuntimeResult::HistoryPage {
            items, next_cursor, ..
        },
        ..
    } = js
        .runtime_response(&ctx, &op, &plan.state, &single)
        .await
        .unwrap()
    else {
        panic!("expected projected history")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "huge");
    assert!(items[0].content.contains("truncated"));
    assert!(next_cursor.is_some());
    validate_runtime_size(&items, MAX_RUNTIME_VALUE_BYTES).unwrap();
}

#[tokio::test]
async fn runtime_goal_native_quickjs_parity() {
    let js = generic_provider(&generic_paths());
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    let fixture: GenericFixture = serde_json::from_str(
        &std::fs::read_to_string(generic_paths().join("shared.json")).unwrap(),
    )
    .unwrap();
    let mut ctx = fixture.context;
    ctx.thread_id = Some("fixture-thread".into());
    let op = RuntimeOperation::Control {
        thread_id: ctx.thread_id.clone(),
        action: "goal".into(),
        value: json!({"action":"read"}),
    };
    for intent in [
        Value::Null,
        json!({"goal":"fixture", "goalStatus":"paused"}),
    ] {
        let reply = RpcReply::Success {
            value: json!({"intent":intent}),
        };
        let a = native
            .runtime_response(&ctx, &op, &Value::Null, &reply)
            .await
            .unwrap();
        let b = js
            .runtime_response(&ctx, &op, &Value::Null, &reply)
            .await
            .unwrap();
        assert_eq!(a, b);
        let RuntimeStep::Complete {
            result:
                RuntimeResult::Goal {
                    thread_id,
                    intent: actual,
                },
            ..
        } = a
        else {
            panic!("goal result required")
        };
        assert_eq!(Some(thread_id), ctx.thread_id);
        assert_eq!(serde_json::to_value(actual).unwrap(), intent);
    }
}

#[tokio::test]
async fn thread_history_capability_native_quickjs_parity() {
    let js = generic_provider(&generic_paths());
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    let mut ctx = RuntimeContext::default();
    ctx.epoch = 1;
    ctx.tab_id = "fixture-tab".into();
    ctx.binding_nonce = "fixture-nonce".into();
    ctx.cwd = "/fixture".into();
    ctx.host_version = "1.0.0".into();
    let op = RuntimeOperation::ForkThread {
        source_thread_id: "source-thread".into(),
        turn_id: "turn-two".into(),
    };
    let mut bound_ctx = ctx.clone();
    bound_ctx.thread_id = Some("already-bound".into());
    assert!(native.runtime_plan(&bound_ctx, &op).await.is_err());
    assert!(js.runtime_plan(&bound_ctx, &op).await.is_err());
    let native_plan = native.runtime_plan(&ctx, &op).await.unwrap();
    let quickjs_plan = js.runtime_plan(&ctx, &op).await.unwrap();
    assert_eq!(native_plan, quickjs_plan);
    let turns_reply = RpcReply::Success {
        value: json!({"data":[{"id":"turn-two","status":"completed"}]}),
    };
    let native_step = native
        .runtime_response(&ctx, &op, &native_plan.state, &turns_reply)
        .await
        .unwrap();
    let quickjs_step = js
        .runtime_response(&ctx, &op, &quickjs_plan.state, &turns_reply)
        .await
        .unwrap();
    assert_eq!(native_step, quickjs_step);
    let RuntimeStep::Continue {
        plan: next_plan, ..
    } = native_step
    else {
        panic!("fork boundary lookup must continue to thread/fork")
    };
    assert_eq!(
        next_plan.action,
        RuntimeAction::Request {
            call: crate::agent_runtime::RpcCall {
                target: None,
                method: "thread/fork".into(),
                params: json!({"threadId":"source-thread","lastTurnId":"turn-two"}),
            },
        }
    );
    let fork_reply = RpcReply::Success {
        value: json!({"thread":{"id":"fork-thread"}}),
    };
    let native_step = native
        .runtime_response(&ctx, &op, &next_plan.state, &fork_reply)
        .await
        .unwrap();
    let quickjs_step = js
        .runtime_response(&ctx, &op, &next_plan.state, &fork_reply)
        .await
        .unwrap();
    assert_eq!(native_step, quickjs_step);
    assert!(matches!(
        native_step,
        RuntimeStep::Complete {
            result: RuntimeResult::ThreadForked { .. },
            ..
        }
    ));

    let event = AgentRuntimeEvent::ThreadReverted {
        thread_id: "source-thread".into(),
    };
    let frame = RuntimeFrame::Notification {
        method: "item".into(),
        params: json!({"usageEvent":event}),
    };
    let mut revert_ctx = ctx.clone();
    revert_ctx.thread_id = Some("source-thread".into());
    let native_reduction = native
        .runtime_reduce(&revert_ctx, &Value::Null, std::slice::from_ref(&frame))
        .await
        .unwrap();
    let quickjs_reduction = js
        .runtime_reduce(&revert_ctx, &Value::Null, &[frame])
        .await
        .unwrap();
    assert_eq!(native_reduction, quickjs_reduction);
}

#[tokio::test]
async fn runtime_native_quickjs_optional_evidence_identity_parity() {
    let dir = generic_paths();
    let fixture: GenericFixture =
        serde_json::from_str(&std::fs::read_to_string(dir.join("shared.json")).unwrap()).unwrap();
    let js = generic_provider(&dir);
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    let mut ctx = fixture.context;
    ctx.thread_id = Some("own-thread".into());
    ctx.claimed_sends = vec![ClaimedSend {
        send_id: "send".into(),
        thread_id: "own-thread".into(),
        input: TurnInput {
            text: "payload".into(),
            attachments: Vec::new(),
        },
    }];
    for item_id in [None, Some("user-item")] {
        let mut evidence = json!({"kind":"sendEvidence", "threadId":"own-thread", "sendId":"send", "turnId":"turn"});
        if let Some(id) = item_id {
            evidence["itemId"] = json!(id);
        }
        let frames = vec![
            RuntimeFrame::Notification {
                method: "item".into(),
                params: json!({"id":"other-item", "type":"assistant", "content":"unrelated"}),
            },
            RuntimeFrame::Notification {
                method: "item".into(),
                params: json!({"usageEvent":evidence}),
            },
        ];
        let a = native
            .runtime_reduce(&ctx, &Value::Null, &frames)
            .await
            .unwrap();
        let b = js
            .runtime_reduce(&ctx, &Value::Null, &frames)
            .await
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(b.events.len(), 2);
        assert!(
            matches!(&b.events[0], AgentRuntimeEvent::ChatItem { item, .. } if item.id == "other-item")
        );
        assert!(
            matches!(&b.events[1], AgentRuntimeEvent::SendEvidence { item_id: actual, .. } if actual.as_deref() == item_id)
        );
    }
}
