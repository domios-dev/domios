use super::*;
use codeterm_plugin::{
    agent_runtime::{
        RuntimeAction, RuntimeContext, RuntimeFrame, RuntimeOperation, RuntimeResult, RuntimeStep,
    },
    js_runtime::{JsProvider, PluginManifest, PluginVm},
    provider::AgentProvider,
};

async fn admits_before_network(
    declaration: &RuntimeHttpSse,
    plan: &codeterm_plugin::agent_runtime::RuntimePlan,
) {
    let RuntimeAction::Request { call } = &plan.action else {
        panic!("request required")
    };
    let (client, _messages) = Client::channel(None, 16, 16);
    let (written, receipt) = oneshot::channel();
    let boundary = RuntimeFailure::PrewriteRefused("fixture network boundary".into());
    let expected_reason = boundary.to_string();
    execute(
        &client,
        &reqwest::Client::new(),
        &Url::parse("http://127.0.0.1:12345/").unwrap(),
        declaration,
        "fixture-password",
        &Mutex::new(requests::Requests::default()),
        Write {
            text: json!({"id":1,"method":call.method,"params":call.params}).to_string(),
            guard: Some(Arc::new(move || Err(boundary.clone()))),
            written,
            turn_write: None,
        },
    )
    .await
    .unwrap();
    let actual = receipt.await.unwrap();
    assert!(
        matches!(&actual, Err(RuntimeFailure::PrewriteRefused(reason)) if reason == &expected_reason),
        "{} must pass the actual transport declaration check; actual: {actual:?}",
        call.method,
    );
}

fn recorded_http_provider() -> (Value, JsProvider) {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/runtime-http-sse.json"
    )))
    .unwrap();
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(fixture["source"]["pluginDirectory"].as_str().unwrap());
    let manifest = PluginManifest::from_json_str(
        &std::fs::read_to_string(directory.join("plugin.json")).unwrap(),
    )
    .unwrap();
    let vm = PluginVm::load_with_manifest(
        &manifest.id,
        &directory.join(&manifest.entry),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    (fixture, JsProvider::new(manifest, Arc::new(vm)))
}

#[tokio::test]
async fn http_restart_events_preserve_selected_mapper_and_admit_same_session_model_routes() {
    let (fixture, provider) = recorded_http_provider();
    let capability = provider.agent_runtime().unwrap();
    assert_eq!(
        capability,
        serde_json::from_value::<AgentRuntimeCapability>(fixture["capability"].clone()).unwrap()
    );
    let declaration = capability.http_sse.unwrap();
    let modern = &declaration.alternatives[0];
    let output = format!("{}http://127.0.0.1:12345\n", modern.readiness_prefix);
    let selected = readiness(
        &mut output.as_bytes(),
        &Launch {
            args: vec![],
            password: "fixture-password".into(),
            declaration,
            url: Url::parse("http://127.0.0.1:12345/").unwrap(),
        },
    )
    .await
    .unwrap();
    assert_eq!(selected.routes["v2.session.model"].method, "POST");
    assert_eq!(
        selected.routes["v2.session.model"].path,
        "/api/session/{sessionID}/model"
    );
    let frames: Vec<RuntimeFrame> = serde_json::from_value(fixture["frames"].clone()).unwrap();
    for epoch in [2, 3] {
        let mut context: RuntimeContext =
            serde_json::from_value(fixture["context"].clone()).unwrap();
        context.tab_id = "http-restart-events".into();
        context.epoch = epoch;
        context.binding_nonce = format!("restart-{epoch}");
        context.settings.model = Some("fixture/selected".into());
        let mut raw = Value::Null;
        for (operation, replies, methods) in [
            (
                RuntimeOperation::Initialize,
                vec![
                    fixture["health"].clone(),
                    fixture["providers"].clone(),
                    fixture["integrations"].clone(),
                ],
                vec!["health", "v2.provider.list", "v2.integration.list"],
            ),
            (
                RuntimeOperation::ResumeThread {
                    thread_id: context.thread_id.clone().unwrap(),
                    instructions: "opaque restart".into(),
                    settings: context.settings.clone(),
                },
                vec![fixture["session"].clone(), Value::Null, Value::Null],
                vec![
                    "v2.session.get",
                    "v2.session.model",
                    "v2.session.instructions",
                ],
            ),
        ] {
            let mut plan = provider.runtime_plan(&context, &operation).await.unwrap();
            let mut completed = false;
            for (body, method) in replies.into_iter().zip(methods) {
                let reduced = provider
                    .runtime_reduce(&context, &raw, &frames[..1])
                    .await
                    .unwrap();
                if method == "health" {
                    assert_eq!(reduced.state, Value::Null);
                    assert!(reduced.events.is_empty());
                }
                raw = reduced.state;
                let RuntimeAction::Request { call } = &plan.action else {
                    panic!("request required")
                };
                assert_eq!(call.method, method);
                if method == "v2.session.model" {
                    assert_eq!(
                        call.params,
                        json!({"path":{"sessionID":context.thread_id},"body":{"model":{"providerID":"fixture","id":"selected"}}})
                    );
                }
                admits_before_network(&selected, &plan).await;
                let step = provider
                    .runtime_response(
                        &context,
                        &operation,
                        &plan.state,
                        &codeterm_plugin::agent_runtime::RpcReply::Success {
                            value: json!({"body":body,"headers":{}}),
                        },
                    )
                    .await
                    .unwrap();
                match step {
                    RuntimeStep::Continue { plan: next, .. } => plan = next,
                    RuntimeStep::Complete { result, .. } => {
                        completed = true;
                        match result {
                            RuntimeResult::Initialized { .. } => {
                                assert_eq!(method, "v2.integration.list")
                            }
                            RuntimeResult::ThreadLoaded {
                                thread_id,
                                settings,
                            } => {
                                assert_eq!(thread_id, context.thread_id.clone().unwrap());
                                assert_eq!(settings, context.settings);
                            }
                            _ => panic!("initialization or same-session load required"),
                        }
                    }
                    _ => panic!("restart handshake must succeed"),
                }
            }
            assert!(completed);
        }
    }
}

#[tokio::test]
async fn recorded_stream_reconciles_and_targeted_abort_reaches_idle() {
    use crate::service::agent_runtime::events::{
        fold, observe, translate_with_items, RuntimeEvent, RuntimeMaps,
    };
    use codeterm_plugin::agent_runtime::{
        validate_runtime_plan, validate_runtime_reduction, validate_runtime_step, RpcReply,
        RuntimeActiveTurn, RuntimeTurnDetail, RuntimeTurnState,
    };

    let (fixture, provider) = recorded_http_provider();
    let capability = provider.agent_runtime().unwrap();
    let transport = &capability.http_sse.as_ref().unwrap().alternatives[0];
    let mut context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    let thread = context.thread_id.clone().unwrap();
    let initialize = RuntimeOperation::Initialize;
    let mut plan = provider.runtime_plan(&context, &initialize).await.unwrap();
    for field in ["health", "providers", "integrations"] {
        let reply = RpcReply::Success {
            value: json!({"body":fixture[field],"headers":{}}),
        };
        let step = provider
            .runtime_response(&context, &initialize, &plan.state, &reply)
            .await
            .unwrap();
        validate_runtime_step(&capability, &context, &initialize, &reply, &step).unwrap();
        match step {
            RuntimeStep::Continue { plan: next, .. } => plan = next,
            RuntimeStep::Complete {
                result: RuntimeResult::Initialized { .. },
                ..
            } => {
                assert_eq!(field, "integrations");
            }
            other => panic!("unexpected initialization: {other:?}"),
        }
    }

    let recorded = &fixture["streamAndInterrupt"];
    let frames: Vec<RuntimeFrame> = serde_json::from_value(recorded["frames"].clone()).unwrap();
    let (through_second_start, interrupted) = frames.split_at(frames.len() - 1);
    let mut maps = RuntimeMaps::default();
    let mut state = Value::Null;
    let mut final_rows = std::collections::HashMap::new();
    context.active_turn_id = Some("first".into());
    for frame in through_second_start {
        maps.sync_active_turn(&context);
        let reduced = provider
            .runtime_reduce(&context, &state, std::slice::from_ref(frame))
            .await
            .unwrap();
        validate_runtime_reduction(&context, &reduced).unwrap();
        state = reduced.state;
        for source in reduced.events {
            let event = translate_with_items(&source, &mut maps, false)
                .expect("each recorded update reconciles against host state");
            assert!(!matches!(event, RuntimeEvent::Notice { .. }));
            if let RuntimeEvent::ItemUpdated { item, .. } = &event {
                final_rows.insert(item.kind.clone(), item.clone());
            }
            fold(&mut maps, context.epoch, &event);
            observe(&mut context, &event);
            if matches!(
                event,
                RuntimeEvent::TurnUpdated {
                    state: RuntimeTurnState::Idle,
                    ..
                }
            ) {
                assert!(context.active_turns.is_empty());
                context.active_turn_id = Some("second".into());
            }
        }
    }
    assert_eq!(final_rows["thinking"].content, "Thinking before tool.");
    assert_eq!(
        final_rows["assistant"].content,
        "Fixture response after shell."
    );
    assert_eq!(
        final_rows["tool_call"]
            .tool_result
            .as_deref()
            .unwrap()
            .trim(),
        "fixture-stream"
    );
    assert_eq!(context.active_turn_id.as_deref(), Some("second"));
    assert_eq!(
        context.active_turns,
        vec![RuntimeActiveTurn {
            thread_id: thread.clone(),
            turn_id: "second".into(),
            parent_thread_id: None,
        }]
    );

    let stop = RuntimeOperation::Interrupt {
        thread_id: thread.clone(),
        active_turn_id: context.active_turn_id.clone(),
    };
    let plan = provider.runtime_plan(&context, &stop).await.unwrap();
    validate_runtime_plan(&capability, &context, &stop, &plan).unwrap();
    admits_before_network(transport, &plan).await;
    let reply = RpcReply::Success {
        value: json!({"body":recorded["interrupt"],"headers":{}}),
    };
    let step = provider
        .runtime_response(&context, &stop, &plan.state, &reply)
        .await
        .unwrap();
    validate_runtime_step(&capability, &context, &stop, &reply, &step).unwrap();
    assert!(
        matches!(step, RuntimeStep::Complete { result: RuntimeResult::Done, events: Some(events), .. } if events.is_empty())
    );
    let reduced = provider
        .runtime_reduce(&context, &state, interrupted)
        .await
        .unwrap();
    validate_runtime_reduction(&context, &reduced).unwrap();
    let [source] = reduced.events.as_slice() else {
        panic!("one interrupted completion expected")
    };
    let event = translate_with_items(source, &mut maps, false).unwrap();
    assert!(matches!(&event, RuntimeEvent::TurnUpdated {
        thread_id, turn_id: Some(turn), state: RuntimeTurnState::Idle,
        detail: Some(RuntimeTurnDetail::Interrupted), ..
    } if thread_id == &thread && turn == "second"));
    fold(&mut maps, context.epoch, &event);
    observe(&mut context, &event);
    assert!(context.active_turns.is_empty());
}
