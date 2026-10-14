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
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src/js_runtime/parity_test/fixtures/runtime")
        })
}
fn generic_provider(dir: &Path) -> JsProvider {
    let kinds = [
        "initialize",
        "openThread",
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
        .map(|kind| ((*kind).into(), json!([kind])))
        .collect();
    let manifest=PluginManifest::from_json_str(&json!({"id":"echo-runtime","version":"1.0.0","displayName":"Fixture","capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-engine","args":[]},"requestMethods":methods,"notificationMethods":["item"],"login":{"methods":["browser","device","apiKey"],"terminalCommand":"fixture-engine login"}}},"permissions":{"subprocess":{"allow":["fixture-engine"]}}}).to_string()).unwrap();
    let source = std::fs::read_to_string(dir.join("plugin.js"))
        .expect("generic runtime plugin path must exist");
    JsProvider::new(
        manifest,
        Arc::new(PluginVm::from_source("echo-runtime", source).unwrap()),
    )
}

#[tokio::test]
async fn runtime_native_quickjs_shared_fixture_parity() {
    let dir = generic_paths();
    let fixture: GenericFixture =
        serde_json::from_str(&std::fs::read_to_string(dir.join("shared.json")).unwrap()).unwrap();
    let js = generic_provider(&dir);
    let native = native::NativeRuntime(js.agent_runtime().unwrap());
    assert_eq!(js.agent_runtime(), native.agent_runtime());
    assert_eq!(js.describe().agent_runtime, native.describe().agent_runtime);
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
