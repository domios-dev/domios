use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use std::sync::Arc;

#[test]
fn routing_requires_both_runtime_capability_and_chat_backing() {
    let chat = TabBacking::ChatBackend {
        plugin_id: "arbitrary-provider".into(),
        session_id: "thread".into(),
    };
    assert_eq!(switch_route(true, &chat), SwitchRoute::Runtime);
    assert_eq!(switch_route(false, &chat), SwitchRoute::Pty);
    assert_eq!(switch_route(true, &TabBacking::Pty), SwitchRoute::Pty);
    assert_eq!(switch_route(false, &TabBacking::Pty), SwitchRoute::Pty);
}

#[test]
fn missing_or_partial_confirmation_cannot_confirm_requested_controls() {
    let requested = RuntimeSettings {
        model: Some("target".into()),
        effort: Some("high".into()),
        mode: Some("auto".into()),
        ..Default::default()
    };
    assert!(!confirms(&requested, &RuntimeSettings::default()));
    assert!(!confirms(
        &requested,
        &RuntimeSettings {
            model: Some("old".into()),
            ..requested.clone()
        }
    ));
    assert!(confirms(&requested, &requested));
}

const PROVIDER: &str = "settings-fixture";
const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    let result = {kind:'done'};
    let events = [];
    if (op.kind === 'initialize') result = {kind:'initialized'};
    if (op.kind === 'resumeThread') result = {kind:'threadLoaded',threadId:op.threadId,settings:op.settings};
    if (op.kind === 'readHistory') result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'interrupt') events = [{kind:'turnState',threadId:op.threadId,turnId:op.activeTurnId,state:'idle'}];
    if (op.kind === 'control') {
      if (op.value.model === 'refuse') return {kind:'refused',reason:{code:'provider_refused',message:'settings refused'}};
      events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:op.value}];
    }
    return {kind:'complete',result,events};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;

pub(super) struct RuntimeFixture {
    fixture: ServiceFixture,
    tab: String,
    storage: crate::storage::Storage,
}

impl std::ops::Deref for RuntimeFixture {
    type Target = ServiceFixture;

    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        engine::stop(&self.tab);
        crate::service::session_persist::stop_worker(self.fixture._app.handle())
            .expect("stop runtime fixture persistence worker");
    }
}

pub(super) fn runtime_fixture() -> (RuntimeFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: "own-thread".into(),
    };
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.agent_session_model = Some("prior".into());
    tab.cwd = Some(env!("CARGO_MANIFEST_DIR").into());
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    let storage = crate::storage::Storage::open(
        &fixture.temp.path().join("runtime-desk"),
        crate::storage::StorageConfig::default(),
    )
    .unwrap();
    crate::service::session_persist::install_storage_desk_adapter(
        fixture._app.handle(),
        crate::session::store::StorageDeskAdapter::from_storage(storage.clone()),
    )
    .unwrap();
    let methods = [
        "initialize",
        "resumeThread",
        "readHistory",
        "control",
        "interrupt",
    ]
    .into_iter()
    .map(|kind| (kind.to_owned(), json!([kind])))
    .collect::<serde_json::Map<String, serde_json::Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"node","args":[format!("{}/../tests/fixtures/agent-runtime/fake-peer.mjs",env!("CARGO_MANIFEST_DIR"))]},"requestMethods":methods,"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["node"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).unwrap()),
        )));
    super::super::authority::record_spawn(&fixture.service, &id, PROVIDER, "own-nonce").unwrap();
    (
        RuntimeFixture {
            fixture,
            tab: id.clone(),
            storage,
        },
        id,
    )
}

#[test]
fn runtime_control_confirmation_updates_recorded_model_and_next_turn_settings() {
    let (fixture, id) = runtime_fixture();
    let outcome = fixture
        .service
        .agent_switch_model_with_restart_fallback(
            &id,
            "target",
            Some("high"),
            Some(RuntimeMode::Auto),
            crate::service::agent_runtime::model::ModelSwitchOptions {
                force: false,
                restart_resume: false,
            },
        )
        .unwrap();
    assert_eq!(
        outcome.mechanism,
        crate::agent::provider::ModelSwitchMode::Live
    );
    assert!(
        matches!(outcome.confirmation, ModelSwitchConfirmation::Observed { model, .. } if model == "target")
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("target"));
        assert_eq!(
            tab.reasoning_effort_identity.actual.as_deref(),
            Some("high")
        );
        assert_eq!(tab.structured_runtime_mode.as_deref(), Some("auto"));
        assert!(tab.pending_model_selection.is_none());
        assert_eq!(tab.restorable_launch.model.as_deref(), Some("target"));
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["model"],
            "target"
        );
    }
    crate::service::session_persist::flush_session_persistence(fixture._app.handle()).unwrap();
    let persisted =
        crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
            .snapshot()
            .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    assert_eq!(
        restored
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .model
            .as_deref(),
        Some("target")
    );
    let session = engine::for_tab(&id).unwrap();
    session.context.lock().active_turn_id = Some("working-turn".into());
    fixture
        .service
        .agent_switch_model_with_options(&id, "mid-turn", Some("low"), false)
        .unwrap();
    assert_eq!(
        session.context.lock().settings.model.as_deref(),
        Some("mid-turn")
    );
    let refusal = fixture
        .service
        .agent_switch_model_with_options(&id, "refuse", None, false)
        .unwrap_err();
    assert!(matches!(
        refusal,
        ModelSwitchError::RuntimeControlRefused(_)
    ));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_model
            .as_deref(),
        Some("mid-turn")
    );
    assert_eq!(
        session.context.lock().settings.model.as_deref(),
        Some("mid-turn")
    );
    engine::stop(&id);
}
