use super::*;
use crate::js_runtime::{PluginManifest, PluginVm};
use crate::provider::AgentProvider;
use std::sync::Arc;

fn context() -> RuntimeContext {
    serde_json::from_value(json!({"epoch":1,"tabId":"own-tab","bindingNonce":"own-nonce","cwd":"/project","hostVersion":"1.0.0","claimedSends":[],"settings":{},"providerSettings":{}})).unwrap()
}

fn provider(source: &str) -> JsProvider {
    let manifest = PluginManifest::from_json_str(&json!({"id":"runtime-hook-fixture","version":"1.0.0","displayName":"Fixture","capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-engine","args":[]},"requestMethods":{"initialize":["handshake"],"submitTurn":["submit"],"answer":[],"interrupt":["interrupt"]},"notificationMethods":["changed"]}},"permissions":{"subprocess":{"allow":["fixture-engine"]}}}).to_string()).unwrap();
    JsProvider::new(
        manifest,
        Arc::new(PluginVm::from_source("runtime-hook-fixture", source.into()).unwrap()),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_hooks_refuse_schema_size_and_unlisted_method() {
    let ctx = context();
    let op = RuntimeOperation::Initialize;
    for (source, expected) in [
        ("module.exports={runtimePlan(){return {action:{kind:'request',call:{method:'handshake',params:null}},state:null,foreign:true}}}", RuntimeFailureCode::InvalidSchema),
        ("module.exports={runtimePlan(){return {action:{kind:'request',call:{method:'handshake',params:null}},state:'x'.repeat(262144)}}}", RuntimeFailureCode::SizeLimit),
        ("module.exports={runtimePlan(){return {action:{kind:'request',call:{method:'undeclared',params:null}},state:null}}}", RuntimeFailureCode::MethodNotAllowed),
        ("module.exports={runtimePlan(){return null}}", RuntimeFailureCode::InvalidSchema),
        ("module.exports={}", RuntimeFailureCode::HookFailed),
    ] {
        assert_eq!(provider(source).runtime_plan(&ctx, &op).await.unwrap_err().code, expected);
    }
    let vm = provider("module.exports={runtimePlan(){while(true){}}}");
    let vm = vm.vm.clone();
    let failure = tokio::task::spawn_blocking(move || {
        crate::js_runtime::vm::call_runtime_hook::<RuntimePlan>(
            &vm,
            "runtimePlan",
            &[],
            std::time::Instant::now() + std::time::Duration::from_millis(20),
        )
    })
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(failure.code, RuntimeFailureCode::Deadline);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_hooks_release_async_runtime_while_quickjs_is_blocked() {
    use crate::js_runtime::call_gate;
    let entered = "runtime-hook-entered";
    let release = "runtime-hook-release";
    call_gate::reset(entered);
    call_gate::reset(release);
    let js = Arc::new(provider("module.exports={runtimePlan(){__runtime_test_gate__.testGateOpen('runtime-hook-entered');__runtime_test_gate__.testGateBlock('runtime-hook-release');return {action:{kind:'request',call:{method:'handshake',params:null}},state:null}}}"));
    let worker = tokio::spawn(async move {
        js.runtime_plan(&context(), &RuntimeOperation::Initialize)
            .await
    });
    let entered_ok = tokio::task::spawn_blocking(move || {
        call_gate::wait_opened(entered, std::time::Duration::from_secs(1))
    })
    .await
    .unwrap();
    call_gate::open(release);
    assert!(entered_ok);
    assert!(worker.await.unwrap().is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_hooks_keep_state_owned_by_exact_runtime_caller() {
    let js = provider("module.exports={runtimeReduce(ctx,state){return {state:{tab:ctx.tabId,count:(state.count||0)+1},events:[]}}}");
    let a = context();
    let mut b = context();
    b.tab_id = "other-tab".into();
    b.binding_nonce = "other-nonce".into();
    let result_a = js
        .runtime_reduce(&a, &json!({"count":4}), &[])
        .await
        .unwrap();
    let result_b = js
        .runtime_reduce(&b, &json!({"count":0}), &[])
        .await
        .unwrap();
    assert_eq!(result_a.state, json!({"tab":"own-tab","count":5}));
    assert_eq!(result_b.state, json!({"tab":"other-tab","count":1}));
}

#[test]
fn runtime_hook_validation_refuses_retargeting_and_submit_replay() {
    let js = provider("module.exports={}");
    let cap = js.agent_runtime().unwrap();
    let mut ctx = context();
    ctx.thread_id = Some("own-thread".into());
    let op: RuntimeOperation = serde_json::from_value(json!({"kind":"submitTurn","threadId":"own-thread","sendId":"send-1","input":{"text":"payload","attachments":[]},"settings":{}})).unwrap();
    let continuation: RuntimeStep = serde_json::from_value(json!({"kind":"continue","plan":{"action":{"kind":"request","call":{"method":"submit","params":{}}},"state":null},"beforeAcceptance":true})).unwrap();
    assert!(validate_runtime_step(
        &cap,
        &ctx,
        &op,
        &RpcReply::Success { value: Value::Null },
        &continuation
    )
    .is_err());
    assert!(validate_runtime_step(
        &cap,
        &ctx,
        &op,
        &RpcReply::Error {
            error: RpcFailure {
                code: -1,
                message: "refused".into(),
                data: None
            }
        },
        &continuation
    )
    .is_ok());
    let wrong: RuntimeReduction = serde_json::from_value(json!({"state":null,"events":[{"kind":"turnState","threadId":"foreign-thread","state":"idle"}]})).unwrap();
    assert!(validate_runtime_reduction(&ctx, &wrong).is_err());
    let unknown_send: RuntimeReduction = serde_json::from_value(json!({"state":null,"events":[{"kind":"sendEvidence","threadId":"own-thread","sendId":"unclaimed","turnId":"turn-1"}]})).unwrap();
    assert!(validate_runtime_reduction(&ctx, &unknown_send).is_err());
    ctx.pending_request = Some(PendingRuntimeRequest {
        request_id: RpcId::Number(7),
        method: "approval".into(),
        params: Value::Null,
    });
    let answer = RuntimeOperation::Answer {
        request_id: RpcId::Number(7),
        answer: PromptAnswer::Cancel,
    };
    let foreign = RuntimePlan {
        action: RuntimeAction::Reply {
            request_id: RpcId::String("7".into()),
            result: RpcReply::Success { value: Value::Null },
        },
        state: Value::Null,
    };
    assert!(validate_runtime_plan(&cap, &ctx, &answer, &foreign).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_hooks_refuse_host_io_at_bundle_init_and_during_hook() {
    let js =
        provider("const retained=host.envGet('PATH'); module.exports={runtimePlan(){return {}}}");
    let error = js
        .runtime_plan(&context(), &RuntimeOperation::Initialize)
        .await
        .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::HookFailed);
    assert!(error.message.contains("envGet"));
    let swallowed=provider("try{host.envGet('PATH')}catch(_){};module.exports={runtimePlan(){return {action:{kind:'request',call:{method:'handshake',params:null}},state:null}}}");
    let error = swallowed
        .runtime_plan(&context(), &RuntimeOperation::Initialize)
        .await
        .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::HookFailed);
    assert!(error.message.contains("envGet"));
    let js = provider("module.exports={runtimePlan(){host.exec({bin:'anything'});return {}}}");
    let error = js
        .runtime_plan(&context(), &RuntimeOperation::Initialize)
        .await
        .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::HookFailed);
    assert!(error.message.contains("exec"));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_hooks_refuse_plugin_receipt_time_and_validate_questions_rates() {
    let mut ctx = context();
    ctx.thread_id = Some("own-thread".into());
    let js=provider("module.exports={runtimeReduce(){return {state:null,events:[{kind:'turnState',threadId:'own-thread',state:'idle',at:1}]}}}");
    assert_eq!(
        js.runtime_reduce(&ctx, &Value::Null, &[])
            .await
            .unwrap_err()
            .code,
        RuntimeFailureCode::InvalidSchema
    );
    let js=provider("module.exports={runtimeReduce(){return {state:null,events:[{kind:'rateLimits',rateLimits:[{window:'primary',usedPercent:101}]}]}}}");
    assert_eq!(
        js.runtime_reduce(&ctx, &Value::Null, &[])
            .await
            .unwrap_err()
            .code,
        RuntimeFailureCode::InvalidSchema
    );
    let js=provider("module.exports={runtimeReduce(){return {state:null,events:[{kind:'rateLimits',rateLimits:[{window:'primary',usedPercent:25}]}]}}}");
    assert!(js.runtime_reduce(&ctx, &Value::Null, &[]).await.is_ok());
    let op:RuntimeOperation=serde_json::from_value(json!({"kind":"answerQuestion","threadId":"own-thread","itemId":"question-item","answers":[{"questionId":"q1","values":["native-id"]}]})).unwrap();
    let local = RuntimePlan {
        action: RuntimeAction::QuestionAnswer {
            input: TurnInput {
                text: "native-id".into(),
                attachments: vec![],
            },
        },
        state: Value::Null,
    };
    assert!(validate_runtime_plan(&js.agent_runtime().unwrap(), &ctx, &op, &local).is_ok());
    assert!(validate_runtime_plan(
        &js.agent_runtime().unwrap(),
        &context(),
        &RuntimeOperation::Initialize,
        &local
    )
    .is_err());
    let direct = RuntimePlan {
        action: RuntimeAction::Request {
            call: RpcCall {
                target: None,
                method: "handshake".into(),
                params: Value::Null,
            },
        },
        state: Value::Null,
    };
    assert!(validate_runtime_plan(&js.agent_runtime().unwrap(), &ctx, &op, &direct).is_err());
    let event:AgentRuntimeEvent=serde_json::from_value(json!({"kind":"turnState","threadId":"own-thread","state":"idle","startedAt":10,"completedAt":20})).unwrap();
    let enriched = HostAgentRuntimeEvent { at: 30, event };
    assert!(validate_host_runtime_event(&ctx, &enriched).is_ok());
    let wire = serde_json::to_value(&enriched).unwrap();
    assert_eq!(wire["at"], json!(30));
    assert!(serde_json::from_value::<AgentRuntimeEvent>(wire.clone()).is_err());
    assert_eq!(
        serde_json::from_value::<HostAgentRuntimeEvent>(wire).unwrap(),
        enriched
    );
}

#[test]
fn runtime_active_turns_and_interrupt_targets_are_confined() {
    let js = provider("module.exports={}");
    let cap = js.agent_runtime().unwrap();
    let mut ctx = context();
    ctx.thread_id = Some("root".into());
    ctx.active_turn_id = Some("root-turn".into());
    ctx.thread_parents = vec![RuntimeThreadParent {
        thread_id: "child".into(),
        parent_thread_id: "root".into(),
    }];
    ctx.active_turns = vec![RuntimeActiveTurn {
        thread_id: "child".into(),
        turn_id: "child-turn".into(),
        parent_thread_id: Some("root".into()),
    }];
    assert!(validate_runtime_context(&ctx).is_ok());
    let op = RuntimeOperation::Interrupt {
        thread_id: "root".into(),
        active_turn_id: Some("root-turn".into()),
    };
    let call = RpcCall {
        method: "interrupt".into(),
        params: Value::Null,
        target: Some(RuntimeCallTarget {
            thread_id: "child".into(),
            turn_id: Some("child-turn".into()),
        }),
    };
    assert!(validate_runtime_call(&cap, &ctx, &op, &call).is_ok());
    let foreign = RpcCall {
        target: Some(RuntimeCallTarget {
            thread_id: "foreign".into(),
            turn_id: Some("other-turn".into()),
        }),
        ..call.clone()
    };
    assert!(validate_runtime_call(&cap, &ctx, &op, &foreign).is_err());
    let missing = RpcCall {
        target: None,
        ..call
    };
    assert!(validate_runtime_call(&cap, &ctx, &op, &missing).is_err());
    ctx.active_turns[0].parent_thread_id = Some("child".into());
    assert!(validate_runtime_context(&ctx).is_err());
}

#[test]
fn runtime_event_batch_accepts_ordered_rooted_descendants_only() {
    let mut ctx = context();
    ctx.thread_id = Some("root".into());
    let event = |thread: &str, parent: &str| {
        HostAgentRuntimeEvent {at:1,event:serde_json::from_value(json!({"kind":"turnState","threadId":thread,"turnId":format!("{thread}-turn"),"parentThreadId":parent,"state":"working"})).unwrap()}
    };
    let child = event("child", "root");
    let grandchild = event("grandchild", "child");
    assert!(validate_host_runtime_events(&ctx, &[child.clone(), grandchild.clone()]).is_ok());
    assert!(validate_host_runtime_events(&ctx, &[grandchild, child]).is_err());
    assert!(validate_host_runtime_events(&ctx, &[event("child", "foreign")]).is_err());
}

#[test]
fn runtime_completed_parent_preserves_active_descendant_without_interrupting_parent() {
    let cap = provider("module.exports={}").agent_runtime().unwrap();
    let mut ctx = context();
    ctx.thread_id = Some("root".into());
    ctx.thread_parents = vec![
        RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "root".into(),
        },
        RuntimeThreadParent {
            thread_id: "grandchild".into(),
            parent_thread_id: "child".into(),
        },
    ];
    ctx.active_turns = vec![RuntimeActiveTurn {
        thread_id: "grandchild".into(),
        turn_id: "grandchild-turn".into(),
        parent_thread_id: Some("child".into()),
    }];
    assert!(validate_runtime_context(&ctx).is_ok());
    let event:AgentRuntimeEvent=serde_json::from_value(json!({"kind":"turnState","threadId":"grandchild","parentThreadId":"child","turnId":"grandchild-turn","state":"working"})).unwrap();
    assert!(validate_host_runtime_event(&ctx, &HostAgentRuntimeEvent { at: 1, event }).is_ok());
    let op = RuntimeOperation::Interrupt {
        thread_id: "root".into(),
        active_turn_id: None,
    };
    let call = |thread: &str, turn: &str| RpcCall {
        method: "interrupt".into(),
        params: Value::Null,
        target: Some(RuntimeCallTarget {
            thread_id: thread.into(),
            turn_id: Some(turn.into()),
        }),
    };
    assert!(validate_runtime_call(&cap, &ctx, &op, &call("grandchild", "grandchild-turn")).is_ok());
    assert!(validate_runtime_call(&cap, &ctx, &op, &call("child", "completed-turn")).is_err());
    ctx.thread_parents[0].parent_thread_id = "grandchild".into();
    assert!(validate_runtime_context(&ctx).is_err());
}

#[test]
fn runtime_history_questions_are_typed_bounded_and_thread_confined() {
    let mut ctx = context();
    ctx.thread_id = Some("own-thread".into());
    let cap = provider("module.exports={}").agent_runtime().unwrap();
    let op: RuntimeOperation =
        serde_json::from_value(json!({"kind":"readHistory","threadId":"own-thread"})).unwrap();
    let reply: RpcReply = serde_json::from_value(json!({"kind":"success","value":null})).unwrap();
    let question = json!({"threadId":"own-thread","itemId":"question-item","questions":[{"id":"q1","text":"choose","options":[{"value":"native-id","label":"label"}]}],"response":"message","resolved":true});
    let step = |questions: Value| json!({"kind":"complete","result":{"kind":"historyPage","threadId":"own-thread","items":[],"openItems":[],"questions":questions}});
    let good: RuntimeStep = serde_json::from_value(step(json!([question.clone()]))).unwrap();
    assert!(validate_runtime_step(&cap, &ctx, &op, &reply, &good).is_ok());
    assert_eq!(
        serde_json::to_value(&good).unwrap()["result"]["questions"][0],
        question
    );
    let mut wrong_thread = question.clone();
    wrong_thread["threadId"] = json!("foreign-thread");
    let mut too_many = question.clone();
    too_many["questions"] = json!(vec![question["questions"][0].clone(); 33]);
    for questions in [
        json!([wrong_thread]),
        json!([question.clone(), question.clone()]),
        json!([too_many]),
        json!(vec![question.clone(); 1025]),
    ] {
        let bad: RuntimeStep = serde_json::from_value(step(questions)).unwrap();
        assert!(validate_runtime_step(&cap, &ctx, &op, &reply, &bad).is_err());
    }
    let mut unknown = question;
    unknown["kind"] = json!("question");
    assert!(serde_json::from_value::<RuntimeStep>(step(json!([unknown]))).is_err());
}
