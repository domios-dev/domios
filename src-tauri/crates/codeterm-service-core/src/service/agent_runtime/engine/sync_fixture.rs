use super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;

pub struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        test_support::stop_test(&self.0);
    }
}

pub const RUNTIME_FIXTURE_SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const method = {initialize:'fixture/initialize',resumeThread:'fixture/resume',readHistory:'fixture/history',submitTurn:'fixture/submit'}[op.kind];
    return {action:{kind:'request',call:{method,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') return {kind:'complete',result:{kind:'initialized'}};
    if (op.kind === 'resumeThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:op.threadId,settings:op.settings}};
    if (op.kind === 'readHistory') return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:[],openItems:[],order:op.order}};
    return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:reply.value.turnId}};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;

pub fn runtime_fixture() -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_with(RUNTIME_FIXTURE_SOURCE)
}

pub fn runtime_fixture_with(
    source: &str,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_with_notifications(source, &[])
}

pub fn runtime_fixture_with_notifications(
    source: &str,
    notifications: &[&str],
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_options(source, notifications, false, true, false, false)
}

pub fn runtime_fixture_host_approvals() -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup)
{
    runtime_fixture_host_approvals_with(RUNTIME_FIXTURE_SOURCE)
}

pub fn runtime_fixture_host_approvals_with(
    source: &str,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_options(source, &[], false, true, false, true)
}

pub fn runtime_fixture_with_stored_history(
    source: &str,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_options(source, &[], true, true, false, false)
}

pub fn runtime_fixture_without_remote_history(
    source: &str,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_options(source, &[], true, false, false, false)
}

pub fn runtime_fixture_turn_scoped(
    source: &str,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    runtime_fixture_options(source, &[], false, true, true, false)
}

fn runtime_fixture_options(
    source: &str,
    notifications: &[&str],
    stored_history: bool,
    remote_history: bool,
    turn_scoped: bool,
    host_approvals: bool,
) -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
    const PROVIDER: &str = "sync-send-fixture";
    let (mut mux, tab_id) = MultiplexerState::new();
    let nonce = uuid::Uuid::new_v4().to_string();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some(nonce.clone());
    tab.reported_session_binding_nonce = Some(nonce.clone());
    tab.cwd = Some(
        crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: "own-thread".into(),
    };
    let fixture = ServiceFixture::new(mux);

    let mut manifest = json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","storedHistory":stored_history,"command":{"binary":"fixture-cli","args":[]},
                "login":{"methods":["browser","device","apiKey"]},
                "requestMethods":{"control":["fixture/control"],"initialize":["fixture/initialize"],"openThread":["fixture/open"],"resumeThread":["fixture/resume"],"readHistory":["fixture/history"],"submitTurn":["fixture/submit"]},"notificationMethods":notifications}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    });
    if turn_scoped {
        manifest["capabilities"]["agentRuntime"]["turnScopedRequests"] = json!(["submitTurn"]);
        manifest["capabilities"]["agentRuntime"]["interrupt"] = json!({"beforeFirstTurn":true});
        manifest["capabilities"]["agentRuntime"]["requestMethods"]["interrupt"] =
            json!(["fixture/cancel"]);
    }
    if host_approvals {
        manifest["capabilities"]["agentRuntime"]["hostApprovals"] = json!(true);
    }
    if !remote_history {
        manifest["capabilities"]["agentRuntime"]["requestMethods"]
            .as_object_mut()
            .unwrap()
            .remove("readHistory");
    }
    let manifest = PluginManifest::from_json_str(&manifest.to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, source.into()).unwrap()),
        )));
    let (peer, cleanup) = attach_runtime_peer(&fixture, &tab_id);
    (fixture, tab_id, peer, cleanup)
}

pub fn attach_runtime_peer(
    fixture: &ServiceFixture,
    tab_id: &str,
) -> (tokio::io::DuplexStream, Cleanup) {
    let tab = fixture.service.mux.read().get_tab(tab_id).unwrap().clone();
    let provider = tab.agent_type.unwrap();
    let nonce = tab.session_binding_nonce.unwrap();
    let thread = tab.agent_session_id.unwrap();
    super::super::authority::record_spawn(&fixture.service, tab_id, &provider, &nonce).unwrap();
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(&provider)
        .unwrap();
    let (process, peer) = {
        let (host, peer) = tokio::io::duplex(65536);
        let (reader, writer) = tokio::io::split(host);
        (
            Arc::new(Process::for_test(super::super::rpc::Client::attach(
                reader, writer,
            ))),
            peer,
        )
    };
    let mut state = RuntimeState::unloaded(Some(thread.clone()));
    let epoch = state.begin();
    assert!(state.load(epoch, &thread));
    let session = Arc::new(Session {
        account: Mutex::new("default".into()),
        invocation: Mutex::new(None),
        update_generation: std::sync::atomic::AtomicU64::new(0),
        update_recovery_available: std::sync::atomic::AtomicBool::new(false),
        provider,
        adapter: Mutex::new(Some(adapter)),
        context: Mutex::new(RuntimeContext {
            restricted_cli: None,
            host_owned: Default::default(),
            epoch,
            tab_id: tab_id.into(),
            binding_nonce: nonce,
            thread_id: Some(thread),
            cwd: crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
            host_version: codeterm_build_info::version().into(),
            provider_settings: json!({}),
            ..Default::default()
        }),
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(Default::default()),
        login: Mutex::new(Default::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        credential_stamp: Mutex::new(None),
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        stream_progress: tokio::sync::watch::channel(0).0,
        turn_requests: Mutex::new(HashMap::new()),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(super::super::events::RuntimeMaps::default()),
        provider_operation_failures: std::sync::atomic::AtomicU32::new(0),
        bookkeeping_warning_emitted: std::sync::atomic::AtomicBool::new(false),
        transcript_io: Default::default(),
        prompts: Mutex::new(HashMap::new()),
        prompts_cancelled: std::sync::atomic::AtomicBool::new(false),
        process: crate::service::agent_runtime::process_slot::ProcessSlot::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
        history_retry: Mutex::new(None),
        capacity_retry: Mutex::new(Default::default()),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        reverted_thread: Mutex::new(None),
    });
    SESSIONS.lock().insert(tab_id.into(), session);
    let cleanup = Cleanup(tab_id.into());
    (peer, cleanup)
}
