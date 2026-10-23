use super::super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::{chat_send_transaction::ChatSendState, test_fixture::ServiceFixture};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(super) struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        test_support::stop_test(&self.0);
    }
}

pub(super) fn runtime_fixture() -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
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

    let manifest = PluginManifest::from_json_str(&json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                "requestMethods":{"initialize":["fixture/initialize"],"resumeThread":["fixture/resume"],"readHistory":["fixture/history"],"submitTurn":["fixture/submit"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let source = r#"
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
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, source.into()).unwrap()),
        )));
    super::super::super::authority::record_spawn(&fixture.service, &tab_id, PROVIDER, &nonce)
        .unwrap();
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(PROVIDER)
        .unwrap();
    let (process, peer) = crate::async_runtime::block_on(async {
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
        update_generation: std::sync::atomic::AtomicU64::new(0),
        update_recovery_available: std::sync::atomic::AtomicBool::new(false),
        provider: PROVIDER.into(),
        adapter: Mutex::new(Some(adapter)),
        context: Mutex::new(RuntimeContext {
            host_owned: Default::default(),
            epoch,
            tab_id: tab_id.clone(),
            binding_nonce: nonce,
            thread_id: Some("own-thread".into()),
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
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(super::super::super::events::RuntimeMaps::default()),
        provider_operation_failures: std::sync::atomic::AtomicU32::new(0),
        bookkeeping_warning_emitted: std::sync::atomic::AtomicBool::new(false),
        prompts: Mutex::new(HashMap::new()),
        process: crate::service::agent_runtime::process_slot::ProcessSlot::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
        history_retry: Mutex::new(None),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        reverted_thread: Mutex::new(None),
    });
    SESSIONS.lock().insert(tab_id.clone(), session);
    let cleanup = Cleanup(tab_id.clone());
    (fixture, tab_id, peer, cleanup)
}

#[test]
fn restored_and_slept_user_sends_hold_then_flush_once_in_order() {
    use std::sync::atomic::Ordering;
    for asleep in [false, true] {
        let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
        let session = for_tab(&tab_id).unwrap();
        session.state.lock().phase = RuntimePhase::Loading;
        let owner = fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .tab(&tab_id);
        // Drive the existing owner explicitly so readiness is an event under test control.
        owner.running.store(true, Ordering::Release);
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.restored_from_snapshot = true;
            tab.frozen = asleep;
            tab.slept_at_ms = asleep.then_some(42);
            tab.session_binding_nonce = None;
            tab.reported_session_binding_nonce = None;
            tab.boot_stage = None;
        }
        let first = fixture
            .service
            .start_human_chat_send(
                &tab_id,
                crate::service::messenger::HumanInput::composer("first opaque turn"),
                None,
                Some("first-token".into()),
            )
            .unwrap();
        let reservation = fixture
            .service
            .reserve_user_send_with_replay_only(&tab_id, Some("second-token".into()), false)
            .unwrap();
        let second = fixture
            .service
            .prepare_and_admit_user_send(
                reservation,
                crate::service::messenger::HumanInput::composer("second opaque turn"),
                false,
            )
            .unwrap();
        let queued = fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .queued(&tab_id)
            .unwrap();
        assert_eq!(queued.len(), 2);
        let send_ids = queued
            .iter()
            .map(|record| record.send_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(send_ids[0], first.send_id);
        assert_eq!(serde_json::to_value(second).unwrap()["state"], "held");
        for id in &send_ids {
            assert_eq!(
                fixture.service.chat_send_ledger.status(id).unwrap().state,
                ChatSendState::Pending
            );
            assert!(!fixture
                .service
                .chat_send_ledger
                .user_payload_may_have_been_written(id));
        }
        assert!(queued.iter().all(|record| record.held_before_readiness));
        let nonce = {
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&tab_id).unwrap();
            assert!(!tab.frozen);
            assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
            tab.session_binding_nonce.clone().unwrap()
        };
        session.context.lock().binding_nonce = nonce.clone();
        let peer = crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut observed = Vec::new();
            for expected in ["first opaque turn", "second opaque turn"] {
                let mut line = String::new();
                tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                    .await
                    .unwrap()
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "fixture/submit");
                assert_eq!(request["params"]["threadId"], "own-thread");
                assert_eq!(request["params"]["input"]["text"], expected);
                observed.push(request["params"]["sendId"].as_str().unwrap().to_string());
                let response = json!({"id":request["id"],"result":{"turnId":"accepted-turn"}});
                peer.get_mut()
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
            }
            (peer, observed)
        });
        let epoch = session.context.lock().epoch;
        assert!(session.state.lock().load(epoch, "own-thread"));
        bind(&fixture.service, &session, "own-thread").unwrap();
        fixture.service.drain_user_owner(&tab_id).unwrap();
        let (mut peer, observed) = crate::async_runtime::block_on(peer).unwrap();
        assert_eq!(observed, send_ids);
        for (index, id) in send_ids.iter().enumerate() {
            assert_eq!(
                fixture.service.chat_send_ledger.status(id).unwrap().state,
                ChatSendState::Written
            );
            assert!(fixture
                .service
                .chat_send_ledger
                .confirm_rpc_send(
                    &tab_id,
                    "own-thread",
                    &nonce,
                    id,
                    Some(&format!("own-item-{index}")),
                )
                .is_some());
            assert_eq!(
                fixture.service.chat_send_ledger.status(id).unwrap().state,
                ChatSendState::Sent
            );
        }
        assert!(fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .queued(&tab_id)
            .unwrap()
            .is_empty());
        fixture.service.prepare_user_send_target(&tab_id).unwrap();
        fixture.service.drain_user_owner(&tab_id).unwrap();
        use std::{
            pin::Pin,
            task::{Context, Poll, Waker},
        };
        use tokio::io::AsyncBufRead;
        let waker = Waker::noop();
        assert!(matches!(
            Pin::new(&mut peer).poll_fill_buf(&mut Context::from_waker(waker)),
            Poll::Pending
        ));
    }
}

#[test]
fn sync_send_from_plain_thread_writes_one_rpc_turn_and_never_replays_it() {
    assert_sync_send(false, false, false);
}

#[test]
fn runtime_continuation_refuses_a_changed_session_or_nonce_before_write() {
    use crate::service::messenger::Messenger;
    let (fixture, tab_id, mut peer, _cleanup) = runtime_fixture();
    let nonce = for_tab(&tab_id)
        .unwrap()
        .context
        .lock()
        .binding_nonce
        .clone();
    for (thread, nonce) in [
        ("foreign-thread", nonce.as_str()),
        ("own-thread", "foreign-nonce"),
    ] {
        let result = Messenger::deliver_turn_resume(
            &fixture.service,
            &tab_id,
            None,
            thread,
            nonce,
            None,
            "opaque",
        );
        assert!(result.is_err());
        assert!(!crate::service::pty_io::delivery_is_unknown(
            &result.unwrap_err()
        ));
    }
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncRead;
    let mut bytes = [0; 1];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    assert!(matches!(
        Pin::new(&mut peer).poll_read(&mut Context::from_waker(Waker::noop()), &mut buffer),
        Poll::Pending
    ));
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
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
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
    let peer = crate::async_runtime::spawn(async move {
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
                    &crate::service::messenger::AgentInput::Human(
                        crate::service::messenger::HumanInput::composer(PAYLOAD),
                    ),
                    Some(&pending.send_id),
                )
            })
            .join()
            .unwrap()
    })
    .unwrap();
    let _peer = crate::async_runtime::block_on(peer).unwrap();
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
            &crate::service::messenger::AgentInput::Human(
                crate::service::messenger::HumanInput::composer(PAYLOAD)
            ),
            Some(&pending.send_id)
        ),
        Err(crate::service::delivery_error::DeliveryError::SubmitUnconfirmed { .. })
    ));
}

#[test]
fn routed_runtime_envelope_reports_written_then_delivered_without_a_second_rpc() {
    assert_routed_runtime_envelope(false, false);
}

#[test]
fn curator_runtime_envelopes_accept_the_orchestrator_and_host() {
    assert_routed_runtime_envelope(true, false);
    assert_routed_runtime_envelope(true, true);
}

fn assert_routed_runtime_envelope(curator: bool, host: bool) {
    use crate::service::messenger::{Delivery, MessageEnvelope};
    const PAYLOAD: &str = "opaque routed payload";
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
    let source = if curator {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id).unwrap().role_profile = Some("curator".into());
        let workspace = mux.get_active_workspace().unwrap().id.clone();
        mux.create_tab(&workspace).unwrap()
    } else {
        "source".into()
    };
    if curator {
        let service = &fixture.service;
        service
            .set_ext(crate::service::curator::CuratorTabs::default())
            .ok()
            .unwrap();
        let mut plan = crate::plan::Plan::new(
            "Fixture".into(),
            fixture.temp.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&source),
        );
        service.plan_store.save(&mut plan, 1).unwrap();
        service
            .ext::<crate::service::curator::CuratorTabs>()
            .0
            .write()
            .insert(tab_id.clone(), plan.id.clone());
    }
    let mut envelope = if host {
        MessageEnvelope::from_codeterm()
    } else {
        MessageEnvelope::tab("Source", source).from_agent(Some(true))
    };
    envelope.id = Some(uuid::Uuid::new_v4().to_string());
    let expected_text = crate::service::messenger::Messenger::from(envelope.clone())
        .routed_frame(PAYLOAD)
        .to_message();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "fixture/submit");
        assert_eq!(request["params"]["threadId"], "own-thread");
        let text = request["params"]["input"]["text"].as_str().unwrap();
        assert_eq!(text, expected_text);
        let send_id = request["params"]["sendId"].as_str().unwrap().to_string();
        let response = json!({"id":request["id"],"result":{"turnId":"accepted-turn"}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        (peer, send_id)
    });
    let delivery = fixture
        .service
        .gateway_deliver_with_pad_file(
            &tab_id,
            envelope.clone(),
            PAYLOAD,
            crate::service::message_gateway::DeliveryClass::AgentTraffic,
        )
        .unwrap()
        .0;
    let (mut peer, send_id) = crate::async_runtime::block_on(peer).unwrap();
    assert!(matches!(delivery, Delivery::ChatBackend { message_id } if message_id == send_id));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Written
    );
    let nonce = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .session_binding_nonce
        .clone()
        .unwrap();
    assert!(fixture
        .service
        .chat_send_ledger
        .confirm_rpc_send(
            &tab_id,
            "other-thread",
            &nonce,
            &send_id,
            Some("provider-item")
        )
        .is_none());
    let authority = fixture.service.user_send_authority(&tab_id).unwrap();
    let mut foreign_provider = authority.clone();
    foreign_provider.provider_id = "foreign-provider".into();
    assert!(!fixture.service.user_authority_is_current_session(
        &foreign_provider,
        "own-thread",
        &nonce
    ));
    let session = for_tab(&tab_id).unwrap();
    let epoch = session.state.lock().epoch;
    session.state.lock().epoch = epoch + 1;
    assert!(!fixture
        .service
        .user_authority_is_current_session(&authority, "own-thread", &nonce));
    session.state.lock().epoch = epoch;
    assert!(!fixture
        .service
        .user_authority_is_current_session(&authority, "other-thread", &nonce));
    assert!(!fixture.service.user_authority_is_current_session(
        &authority,
        "own-thread",
        "foreign-nonce"
    ));
    assert!(fixture
        .service
        .user_authority_is_current_session(&authority, "own-thread", &nonce));
    assert!(fixture
        .service
        .chat_send_ledger
        .confirm_rpc_send(
            &tab_id,
            "own-thread",
            &nonce,
            &send_id,
            Some("provider-item")
        )
        .is_some());
    let delivery = fixture
        .service
        .gateway_deliver_with_pad_file(
            &tab_id,
            envelope,
            PAYLOAD,
            crate::service::message_gateway::DeliveryClass::AgentTraffic,
        )
        .unwrap()
        .0;
    assert!(matches!(delivery, Delivery::ChatBackend { message_id } if message_id == send_id));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufRead;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(matches!(
        Pin::new(&mut peer).poll_fill_buf(&mut context),
        Poll::Pending
    ));
}

#[test]
fn consecutive_distinct_runtime_envelopes_keep_independent_write_once_receipts() {
    use crate::service::messenger::Delivery;
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut ids = Vec::new();
        for expected in ["first opaque envelope", "second opaque envelope"] {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "fixture/submit");
            assert_eq!(request["params"]["threadId"], "own-thread");
            assert_eq!(
                request["params"]["input"]["text"],
                format!("<domios from=\"system\">{expected}</domios>")
            );
            ids.push(request["params"]["sendId"].as_str().unwrap().to_owned());
            let response = json!({"id":request["id"],"result":{"turnId":"accepted-turn"}});
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
        (peer, ids)
    });
    let mut ids = Vec::new();
    for (token, text) in [
        ("first-token", "first opaque envelope"),
        ("second-token", "second opaque envelope"),
    ] {
        let delivery = super::super::super::transport::send_envelope(
            &fixture.service,
            &tab_id,
            &crate::service::messenger::Messenger::from(
                crate::service::messenger::MessageEnvelope::from_codeterm(),
            )
            .framed_delivery(text),
            token,
            None,
        )
        .unwrap();
        let Delivery::ChatBackend { message_id } = delivery else {
            panic!("accepted RPC must be written")
        };
        ids.push(message_id.clone());
        fixture
            .service
            .restore_durable_user_statuses(&tab_id)
            .unwrap();
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&message_id)
                .unwrap()
                .state,
            ChatSendState::Written
        );
        assert_eq!(
            super::super::super::transport::send_envelope(
                &fixture.service,
                &tab_id,
                &crate::service::messenger::Messenger::from(
                    crate::service::messenger::MessageEnvelope::from_codeterm()
                )
                .framed_delivery(text),
                token,
                None
            )
            .unwrap(),
            Delivery::ChatBackend { message_id }
        );
    }
    assert_ne!(ids[0], ids[1]);
    let (mut peer, observed) = crate::async_runtime::block_on(peer).unwrap();
    assert_eq!(observed, ids);
    let nonce = for_tab(&tab_id)
        .unwrap()
        .context
        .lock()
        .binding_nonce
        .clone();
    for (index, id) in ids.iter().enumerate() {
        assert!(fixture
            .service
            .chat_send_ledger
            .confirm_rpc_send(&tab_id, "foreign-thread", &nonce, id, Some("foreign-item"))
            .is_none());
        fixture
            .service
            .chat_send_ledger
            .confirm_rpc_send(
                &tab_id,
                "own-thread",
                &nonce,
                id,
                Some(&format!("own-item-{index}")),
            )
            .unwrap();
    }
    fixture
        .service
        .restore_durable_user_statuses(&tab_id)
        .unwrap();
    assert!(ids.iter().all(
        |id| fixture.service.chat_send_ledger.status(id).unwrap().state == ChatSendState::Sent
    ));
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufRead;
    assert!(matches!(
        Pin::new(&mut peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[test]
fn provider_catalogue_replacement_keeps_live_runtime_adapter_and_epoch() {
    for active_turn in [None, Some("in-flight-tool-turn")] {
        let (fixture, tab_id, _peer, _cleanup) = runtime_fixture();
        let session = for_tab(&tab_id).unwrap();
        session.context.lock().active_turn_id = active_turn.map(str::to_owned);
        let epoch = session.context.lock().epoch;
        let adapter = session.adapter.lock().clone().unwrap();
        let process = session.process.lock().clone().unwrap();
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("sync-send-fixture"),
        ));
        assert!(!Arc::ptr_eq(
            &adapter,
            &fixture
                .service
                .agent_registry
                .provider_arc("sync-send-fixture")
                .unwrap()
        ));
        crate::async_runtime::block_on(ensure_locked(
            &fixture.service,
            &tab_id,
            "sync-send-fixture",
            &json!({}),
            &session,
            None,
        ))
        .unwrap();
        assert_eq!(session.context.lock().epoch, epoch);
        assert_eq!(
            session.context.lock().active_turn_id.as_deref(),
            active_turn
        );
        assert!(Arc::ptr_eq(
            &adapter,
            session.adapter.lock().as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &process,
            session.process.lock().as_ref().unwrap()
        ));
        assert!(process.ended.borrow().is_none());
        assert!(validate_binding(&fixture.service, &session).is_ok());
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some("foreign".into());
        assert!(matches!(
            validate_binding(&fixture.service, &session),
            Err(RuntimeFailure::Superseded)
        ));
    }
}

#[test]
fn removed_provider_revokes_live_runtime_binding() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture();
    let session = for_tab(&tab).unwrap();
    assert!(validate_binding(&fixture.service, &session).is_ok());
    fixture
        .service
        .agent_registry
        .unregister_provider("sync-send-fixture");
    assert!(matches!(
        validate_binding(&fixture.service, &session),
        Err(RuntimeFailure::Superseded)
    ));
}
