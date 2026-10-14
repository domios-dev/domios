use super::super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::{chat_send_transaction::ChatSendState, test_fixture::ServiceFixture};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        stop(&self.0);
    }
}

#[test]
fn sync_send_from_plain_thread_writes_one_rpc_turn_and_never_replays_it() {
    assert_sync_send(false, false, false);
}

#[test]
fn bookkeeping_failure_records_typed_warning_and_runtime_remains_ready_and_sendable() {
    assert_sync_send(true, false, false);
}

#[test]
fn restart_bookkeeping_failure_records_warning_and_runtime_remains_ready_and_sendable() {
    assert_sync_send(true, true, false);
}

#[test]
fn task_authority_violation_unbinds_agent_and_records_warning_without_blocking_send() {
    assert_sync_send(true, false, true);
}

fn assert_sync_send(bookkeeping_failure: bool, restart_failure: bool, authority_failure: bool) {
    const PROVIDER: &str = "sync-send-fixture";
    const PAYLOAD: &str = "opaque supplied user input";
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
    let manifest = PluginManifest::from_json_str(&json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                "requestMethods":{"submitTurn":["fixture/submit"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let source = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:'fixture/submit',params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:reply.value.turnId}};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, source.into()).unwrap()),
        )));
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(PROVIDER)
        .unwrap();
    let (process, peer) = tauri::async_runtime::block_on(async {
        let (host, peer) = tokio::io::duplex(65536);
        let (reader, writer) = tokio::io::split(host);
        (
            Arc::new(Process::for_test(super::super::super::rpc::Client::attach(
                reader, writer,
            ))),
            peer,
        )
    });
    let mut state = RuntimeState::unloaded(Some("own-thread".into()));
    let epoch = state.begin();
    assert!(state.load(epoch, "own-thread"));
    let session = Arc::new(Session {
        provider: PROVIDER.into(),
        adapter: Mutex::new(Some(adapter)),
        context: Mutex::new(RuntimeContext {
            epoch,
            tab_id: tab_id.clone(),
            binding_nonce: nonce,
            thread_id: Some("own-thread".into()),
            cwd: crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
            host_version: env!("CARGO_PKG_VERSION").into(),
            provider_settings: json!({}),
            ..Default::default()
        }),
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(Default::default()),
        login: Mutex::new(Default::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        hook_state: Mutex::new(Value::Null),
        prompts: Mutex::new(HashMap::new()),
        process: Mutex::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
    });
    SESSIONS.lock().insert(tab_id.clone(), session);
    let _cleanup = Cleanup(tab_id.clone());
    if bookkeeping_failure {
        let mut plan = crate::plan::Plan::new(
            "Warning fixture".into(),
            fixture.temp.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab("owner"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let target_task = if authority_failure {
            fixture
                .service
                .plan_store
                .add_task(
                    &plan.id,
                    crate::service::plan::NewTask {
                        name: "Authority fixture".into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        } else {
            "missing-task".into()
        };
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some(plan.id.clone()),
            task_id: Some(target_task.clone()),
            attempt_id: Some("attempt".into()),
            ..Default::default()
        });
        let own_nonce = for_tab(&tab_id)
            .unwrap()
            .context
            .lock()
            .binding_nonce
            .clone();
        if restart_failure {
            assert!(fixture
                .service
                .authorize_runtime_restart_binding(&tab_id, PROVIDER, &own_nonce, false,)
                .unwrap()
                .is_none());
        } else {
            fixture
                .service
                .finalize_runtime_worker_session(&tab_id, "own-thread", &own_nonce)
                .unwrap();
        }
        let current = for_tab(&tab_id).unwrap();
        assert!(matches!(current.state.lock().phase, RuntimePhase::Ready));
        assert!(current
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .task_binding
                .is_none(),
            authority_failure
        );
        let recorded = fixture.service.plan_store.get(&plan.id).unwrap();
        let warning = recorded
            .strides
            .iter()
            .find(|stride| stride.kind == "runtime_plan_binding_warning")
            .unwrap();
        let payload: Value = serde_json::from_str(&warning.text).unwrap();
        assert_eq!(payload["kind"], "runtime_plan_binding_warning");
        assert_eq!(
            payload["failure_kind"],
            if authority_failure {
                "authority_violation"
            } else {
                "bookkeeping_unavailable"
            }
        );
        assert_eq!(
            payload["stage"],
            if restart_failure {
                "restart_authorization"
            } else {
                "finalization"
            }
        );
        assert_eq!(payload["task_id"], target_task);
        assert_eq!(payload["attempt_id"], "attempt");
        assert_eq!(payload["tab_id"], tab_id);
    }
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab_id, PAYLOAD, None)
        .unwrap();
    let expected_send = pending.send_id.clone();
    let peer = tauri::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "fixture/submit");
        assert_eq!(request["params"]["input"]["text"], PAYLOAD);
        assert_eq!(request["params"]["sendId"], expected_send);
        assert_eq!(request["params"]["threadId"], "own-thread");
        let response = json!({"id":request["id"],"result":{"turnId":"accepted-turn"}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        peer
    });
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                assert!(tokio::runtime::Handle::try_current().is_err());
                super::super::super::transport::send(
                    &fixture.service,
                    &tab_id,
                    PAYLOAD,
                    Some(&pending.send_id),
                )
            })
            .join()
            .unwrap()
    })
    .unwrap();
    let _peer = tauri::async_runtime::block_on(peer).unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&pending.send_id)
            .unwrap()
            .state,
        ChatSendState::Written
    );
    assert!(matches!(
        super::super::super::transport::send(
            &fixture.service,
            &tab_id,
            PAYLOAD,
            Some(&pending.send_id)
        ),
        Err(crate::service::delivery_error::DeliveryError::SubmitUnconfirmed { .. })
    ));
}
