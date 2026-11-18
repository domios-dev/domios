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

#[tokio::test]
async fn http_restart_events_preserve_selected_mapper_and_admit_same_session_model_routes() {
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
    let provider = JsProvider::new(manifest, Arc::new(vm));
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
