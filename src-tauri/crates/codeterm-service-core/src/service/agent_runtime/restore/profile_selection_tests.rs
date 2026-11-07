use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use codeterm_plugin::{
    agent_runtime::{RuntimeAction, RuntimeContext, RuntimeOperation, RuntimeResult, RuntimeStep},
    js_runtime::{JsProvider, PluginManifest, PluginVm},
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

fn register_acp(svc: &crate::service::ServiceContext) {
    let directory = PathBuf::from(crate::test_support::fixture_manifest_dir())
        .join("../../../plugins/provider-acp");
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
    svc.agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
}

fn options(model: &str, effort: &str) -> Value {
    json!([
        {"id":"engine","name":"Model","type":"select","category":"model",
         "currentValue":model,"options":[{"value":"default-model","name":"Default"},
             {"value":"selected-model","name":"Selected"}]},
        {"id":"thinking","name":"Effort","type":"select","category":"thought_level",
         "currentValue":effort,"options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
    ])
}

#[tokio::test]
async fn instance_restore_reapplies_persisted_profile_model_and_non_default_effort() {
    let directory = tempfile::tempdir().unwrap();
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("acp".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.session_binding_nonce = Some("saved-nonce".into());
    tab.reported_session_binding_nonce = Some("saved-nonce".into());
    tab.cwd = Some(directory.path().to_string_lossy().into_owned());
    tab.agent_runtime_config = Some(json!({"launchProfile":"deepseek-harness",
        "model":"selected-model","effort":"low"}));
    tab.restorable_launch.requested_reasoning_effort = Some("low".into());
    tab.restorable_launch.resolved_reasoning_effort = Some("low".into());
    let bytes = serde_json::to_vec(&mux.to_session_snapshot()).unwrap();
    let saved = serde_json::from_slice(&bytes).unwrap();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&saved));
    register_acp(&fixture.service);
    assert!(super::tab(&fixture.service, &id).unwrap());
    let restored = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let settings =
        super::super::model::settings_from_config(restored.agent_runtime_config.as_ref().unwrap());
    assert_eq!(settings.model.as_deref(), Some("selected-model"));
    assert_eq!(settings.effort.as_deref(), Some("low"));
    assert_eq!(
        restored
            .restorable_launch
            .requested_reasoning_effort
            .as_deref(),
        Some("low")
    );
    let context = RuntimeContext {
        epoch: 1,
        tab_id: id.clone(),
        thread_id: Some("saved-thread".into()),
        binding_nonce: restored.session_binding_nonce.unwrap(),
        cwd: restored.cwd.unwrap(),
        host_version: "1.19.1".into(),
        settings: settings.clone(),
        ..Default::default()
    };
    let adapter = fixture.service.agent_registry.provider_arc("acp").unwrap();
    let initialize = RuntimeOperation::Initialize;
    let plan = adapter.runtime_plan(&context, &initialize).await.unwrap();
    let initialized = adapter
        .runtime_response(
            &context,
            &initialize,
            &plan.state,
            &serde_json::from_value(json!({"kind":"success","value":{"protocolVersion":1,
            "agentCapabilities":{"loadSession":true},"authMethods":[]}}))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        initialized,
        RuntimeStep::Complete {
            result: RuntimeResult::Initialized { .. },
            ..
        }
    ));
    let resume = RuntimeOperation::ResumeThread {
        thread_id: "saved-thread".into(),
        instructions: String::new(),
        settings,
    };
    let plan = adapter.runtime_plan(&context, &resume).await.unwrap();
    let RuntimeAction::Request { call } = &plan.action else {
        panic!("expected load request")
    };
    assert_eq!(call.method, "session/load");
    let mut step = adapter
        .runtime_response(
            &context,
            &resume,
            &plan.state,
            &serde_json::from_value(json!({"kind":"success","value":{
            "configOptions":options("default-model", "high")}}))
            .unwrap(),
        )
        .await
        .unwrap();
    for (id, value, model, effort) in [
        ("engine", "selected-model", "selected-model", "high"),
        ("thinking", "low", "selected-model", "low"),
    ] {
        let RuntimeStep::Continue { plan, .. } = step else {
            panic!("expected persisted config selection")
        };
        let RuntimeAction::Request { call } = &plan.action else {
            panic!("expected config request")
        };
        assert_eq!(call.method, "session/set_config_option");
        assert_eq!(
            call.params,
            json!({"sessionId":"saved-thread","configId":id,"value":value})
        );
        step = adapter
            .runtime_response(
                &context,
                &resume,
                &plan.state,
                &serde_json::from_value(
                    json!({"kind":"success","value":{"configOptions":options(model, effort)}}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    }
    let RuntimeStep::Complete {
        result: RuntimeResult::ThreadLoaded {
            thread_id,
            settings,
        },
        ..
    } = step
    else {
        panic!("expected resumed thread")
    };
    assert_eq!(thread_id, "saved-thread");
    assert_eq!(settings.model.as_deref(), Some("selected-model"));
    assert_eq!(settings.effort.as_deref(), Some("low"));
    super::super::authority::revoke(&id);
}
