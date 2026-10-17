use super::super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::{chat_send_transaction::ChatSendState, test_fixture::ServiceFixture};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Cleanup(String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        test_support::stop_test(&self.0);
    }
}

fn runtime_fixture() -> (ServiceFixture, String, tokio::io::DuplexStream, Cleanup) {
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
            .start_chat_send(
                &tab_id,
                "first opaque turn".into(),
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
            .prepare_and_admit_user_send(reservation, "second opaque turn".into(), false)
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
                    PAYLOAD,
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
            PAYLOAD,
            Some(&pending.send_id)
        ),
        Err(crate::service::delivery_error::DeliveryError::SubmitUnconfirmed { .. })
    ));
}

#[test]
fn routed_runtime_envelope_reports_written_then_delivered_without_a_second_rpc() {
    use crate::service::messenger::{Delivery, MessageEnvelope};
    const PAYLOAD: &str = "opaque routed payload";
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
    let mut envelope = MessageEnvelope::tab("Source", "source").from_agent(Some(true));
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
            assert_eq!(request["params"]["input"]["text"], expected);
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
            text,
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
                text,
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
