//! The scripted settings runtime shared by the core controls tests and the lifecycle crate's
//! runtime-routed model switch tests.

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use std::sync::Arc;

pub const PROVIDER: &str = "settings-fixture";

pub const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    if (op.kind === 'control' && op.value.serviceTier !== undefined) return {action:{kind:'request',call:{method:'fixture/settings',params:op}},state:null};
    const target = op.kind === 'interrupt' ? {threadId:op.threadId,...(op.activeTurnId ? {turnId:op.activeTurnId} : {})} : undefined;
    return {action:{kind:'request',call:{method:op.kind,params:op,...(target ? {target} : {})}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    let result = {kind:'done'};
    let events = [];
    if (op.kind === 'initialize') result = {kind:'initialized'};
    if (op.kind === 'resumeThread') result = {kind:'threadLoaded',threadId:op.threadId,settings:op.settings};
    if (op.kind === 'readHistory') result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'submitTurn') {
      result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'restored-turn'};
      events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:reply.value.params.settings}];
    }
    if (op.kind === 'interrupt') {
      events = op.activeTurnId ? [{kind:'turnState',threadId:op.threadId,turnId:op.activeTurnId,state:'idle'}] : [];
      if (ctx.providerSettings && ctx.providerSettings.interruptProbe) events.push({kind:'controlsConfirmed',threadId:op.threadId,settings:{effort:'high'}});
    }
    if (op.kind === 'control') {
      if (op.value.model === 'refuse') return {kind:'refused',reason:{code:'provider_refused',message:'settings refused'}};
      if (op.action === 'mode') events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:{mode:op.value}}];
      else if (op.action === 'config') {
        if (op.value.value === 'refuse') return {kind:'refused',reason:{code:'provider_refused',message:'selection refused'}};
        const options = ctx.settings.configOptions.map(option => option.id !== op.value.id || op.value.value === 'unconfirmed' ? option : {...option,currentValue:op.value.value});
        const model = options.find(option => option.category === 'model').currentValue;
        const effort = options.find(option => option.category === 'thought_level').currentValue;
        events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:{model,effort,configOptions:options}}];
      } else if (op.value.serviceTier === undefined) events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:op.value}];
    }
    return {kind:'complete',result,events};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:frames.map(frame=>({kind:'controlsConfirmed',threadId:frame.params.threadId,settings:frame.params.settings}))}; }
};
"#;

pub struct RuntimeFixture {
    pub fixture: ServiceFixture,
    pub tab: String,
    pub storage: crate::storage::Storage,
}

impl std::ops::Deref for RuntimeFixture {
    type Target = ServiceFixture;

    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        engine::stop_blocking(&self.fixture.service, &self.tab).unwrap();
        crate::service::session_persist::stop_worker(&self.fixture.service)
            .expect("stop runtime fixture persistence worker");
    }
}

pub fn runtime_fixture() -> (RuntimeFixture, String) {
    runtime_fixture_with_interrupt(false)
}

pub fn runtime_fixture_with_interrupt(before_first: bool) -> (RuntimeFixture, String) {
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
    tab.cwd = Some(
        crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    let storage = fixture.install_seeded_storage_desk();
    let methods = [
        "initialize",
        "resumeThread",
        "readHistory",
        "submitTurn",
        "control",
        "interrupt",
    ]
    .into_iter()
    .map(|kind| {
        (
            kind.to_owned(),
            if kind == "control" {
                json!([kind, "fixture/settings"])
            } else {
                json!([kind])
            },
        )
    })
    .collect::<serde_json::Map<String, serde_json::Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "reasoningEfforts":[{"id":"low","displayName":"Low"},{"id":"high","displayName":"High"}],
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"interrupt":if before_first {json!({"beforeFirstTurn":true})} else {serde_json::Value::Null},"runtimeModes":[{"id":"approval-required","displayName":"Approval required"},{"id":"auto-accept-edits","displayName":"Auto accept edits"},{"id":"auto","displayName":"Auto"},{"id":"full-access","displayName":"Full access"}],"serviceTiers":[{"id":"priority","name":"Fast","description":""}],"runtime":"stdio-jsonrpc","command":{"binary":"node","args":[format!("{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs",crate::test_support::fixture_manifest_dir().display())]},"requestMethods":methods,"notificationMethods":["fixture/settings/updated"]}},
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

pub fn tier_selection(fast: bool) -> codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
    codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
        id: fast.then(|| "priority".into()),
    }
}
