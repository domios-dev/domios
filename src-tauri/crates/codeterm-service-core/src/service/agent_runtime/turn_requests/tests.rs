use super::super::engine::test_support::runtime_fixture_turn_scoped;
use super::*;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(in crate::service::agent_runtime) const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    if (op.kind === 'interrupt') return {action:{kind:'notification',notification:{method:'fixture/cancel',params:{threadId:op.threadId},target:{threadId:op.threadId,turnId:op.activeTurnId}}},state:null};
    return {action:{kind:'request',call:{method:'fixture/submit',params:{threadId:op.threadId},target:{threadId:op.threadId,turnId:'turn'}}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'interrupt') return {kind:'complete',result:{kind:'done'}};
    if (reply.kind === 'error') return {kind:'refused',reason:{code:'provider_refused',message:reply.error.message},beforeAcceptance:false};
    return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'turn'},events:[{kind:'turnState',threadId:op.threadId,turnId:'turn',state:'idle',...(reply.value.stopReason === 'cancelled' ? {detail:'interrupted'} : {})}]};
  },
  runtimeReduce(ctx, state, frames) { return {state,events:[]}; }
};
"#;

pub(in crate::service::agent_runtime) fn operation(context: &RuntimeContext) -> RuntimeOperation {
    RuntimeOperation::SubmitTurn {
        thread_id: context.thread_id.clone().unwrap(),
        active_turn_id: None,
        send_id: "send".into(),
        input: TurnInput {
            text: "opaque prompt".into(),
            attachments: Vec::new(),
        },
        settings: RuntimeSettings::default(),
    }
}

pub(in crate::service::agent_runtime) async fn accept(
    svc: &ServiceContext,
    session: &Arc<Session>,
) {
    let context = session.context.lock().clone();
    let thread = context.thread_id.clone().unwrap();
    session.context.lock().claimed_sends.push(ClaimedSend {
        send_id: "send".into(),
        thread_id: thread.clone(),
        input: TurnInput {
            text: "opaque prompt".into(),
            attachments: Vec::new(),
        },
    });
    subscriber::publish_events(
        svc,
        session,
        context.epoch,
        vec![
            AgentRuntimeEvent::TurnState {
                thread_id: thread.clone(),
                turn_id: Some("turn".into()),
                state: RuntimeTurnState::Working,
                detail: None,
                status_line: None,
                started_at: None,
                completed_at: None,
                parent_thread_id: None,
            },
            AgentRuntimeEvent::ChatItem {
                thread_id: thread.clone(),
                item: ChatMsg {
                    id: "user".into(),
                    kind: "user".into(),
                    content: "opaque prompt".into(),
                    ..Default::default()
                },
            },
            AgentRuntimeEvent::SendEvidence {
                thread_id: thread,
                send_id: "send".into(),
                turn_id: "turn".into(),
                item_id: Some("user".into()),
            },
        ],
        1,
    )
    .await
    .unwrap();
}

pub(in crate::service::agent_runtime) async fn completed(session: &Arc<Session>) {
    let mut changes = session.control_changes.subscribe();
    let thread = session.context.lock().thread_id.clone().unwrap();
    loop {
        changes.borrow_and_update();
        if session.completions.lock().contains(&thread, "turn") {
            return;
        }
        changes.changed().await.unwrap();
    }
}

#[tokio::test]
async fn declared_long_turn_accepts_evidence_and_settles_from_late_reply() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let op = operation(&session.context.lock());
    let (accepted, receipt) = tokio::sync::oneshot::channel();
    let submit = async {
        let (result, written) = engine::run_submit(&fixture.service, &session, &process, op).await;
        assert!(written);
        assert!(matches!(
            result,
            Ok(RuntimeResult::TurnAccepted {
                turn_id: Some(_),
                ..
            }) | Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
        accepted.send(()).unwrap();
    };
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        accept(&fixture.service, &session).await;
        receipt.await.unwrap();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(
            session.context.lock().active_turn_id.as_deref(),
            Some("turn")
        );
        peer.get_mut()
            .write_all(
                format!(
                    "{{\"id\":{},\"result\":{{\"stopReason\":\"end_turn\"}}}}\n",
                    request["id"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        completed(&session).await;
        assert!(session.context.lock().active_turn_id.is_none());
        peer
    };
    let ((), _peer) = tokio::join!(submit, server);
}

#[tokio::test]
async fn process_exit_with_a_pending_turn_reply_settles_failed_visibly() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let op = operation(&session.context.lock());
    let (accepted, receipt) = tokio::sync::oneshot::channel();
    let submit = async {
        let (result, _) = engine::run_submit(&fixture.service, &session, &process, op).await;
        assert!(matches!(
            result,
            Ok(RuntimeResult::TurnAccepted { .. }) | Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
        accepted.send(()).unwrap();
    };
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        accept(&fixture.service, &session).await;
        receipt.await.unwrap();
        drop(peer);
        completed(&session).await;
        assert!(session.context.lock().active_turn_id.is_none());
        assert!(session
            .cache
            .lock()
            .snapshot()
            .iter()
            .any(|row| row.content.contains("completion reply failed")));
    };
    tokio::join!(submit, server);
}

#[tokio::test]
async fn stream_window_reports_prewrite_boundary_and_successful_receipt() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let context = session.context.lock().clone();
    let witness = Arc::new(rpc::WriteWitness::default());
    let pending = Arc::new(PendingTurn {
        context,
        send_id: "send".into(),
        settled: AtomicBool::new(false),
        refused: AtomicBool::new(false),
        observed: AtomicBool::new(false),
        had_predecessor: false,
        started_at: 1000,
        order: 1,
        activity: tokio::sync::OnceCell::new(),
        turn: "turn".into(),
        witness: witness.clone(),
        outcome: watch::channel(None).0,
        cancelled: watch::channel(false).0,
    });
    session.turn_requests.lock().insert("send".into(), pending);
    let before = stream_window(&session, vec![1]).await;
    assert_eq!(before.turn_requests[0].after_sequence, None);
    assert!(!before.turn_requests[0].written);
    let process = session.process.lock().clone().unwrap();
    let call = RpcCall {
        target: None,
        method: "fixture/submit".into(),
        params: json!({}),
    };
    let request = process.client.request_turn(&call, None, witness.clone());
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        witness.wait().await.unwrap();
        let after = stream_window(&session, vec![1]).await;
        assert_eq!(after.frame_sequences, vec![1]);
        assert_eq!(after.turn_requests[0].after_sequence, Some(0));
        assert!(after.turn_requests[0].written);
        let context = session.context.lock().clone();
        for (state, at) in [
            (RuntimeTurnState::Working, 123961),
            (RuntimeTurnState::Idle, 124000),
        ] {
            subscriber::publish_events(
                &fixture.service,
                &session,
                context.epoch,
                vec![AgentRuntimeEvent::TurnState {
                    thread_id: "own-thread".into(),
                    turn_id: Some("turn".into()),
                    parent_thread_id: None,
                    state,
                    detail: None,
                    status_line: None,
                    started_at: None,
                    completed_at: None,
                }],
                at,
            )
            .await
            .unwrap();
        }
        assert_eq!(
            session.state.lock().last_turn_duration_ms,
            Some(123000),
            "duration includes the wait before the first own update"
        );
        let pending = session.turn_requests.lock().get("send").cloned().unwrap();
        publish_written_activity(&fixture.service, &session, &pending)
            .await
            .unwrap();
        assert!(
            session.context.lock().active_turn_id.is_none(),
            "a late write observer cannot reopen a completed turn"
        );
        let request: Value = serde_json::from_str(&line).unwrap();
        let response = json!({"id":request["id"],"result":{}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    let (result, _peer) = tokio::join!(request, server);
    assert!(result.is_ok());
}

#[tokio::test]
async fn completion_waits_for_prior_ingress_projection() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let pending = PendingTurn {
        context: session.context.lock().clone(),
        send_id: "send".into(),
        settled: AtomicBool::new(false),
        refused: AtomicBool::new(false),
        observed: AtomicBool::new(false),
        had_predecessor: false,
        started_at: now(),
        order: 1,
        activity: tokio::sync::OnceCell::new(),
        turn: "turn".into(),
        witness: Arc::new(rpc::WriteWitness::default()),
        outcome: watch::channel(None).0,
        cancelled: watch::channel(false).0,
    };
    let completing = async {
        wait_for_updates(&fixture.service, &session, &process, &pending, 7)
            .await
            .unwrap();
        assert!(session
            .cache
            .lock()
            .snapshot()
            .iter()
            .any(|row| row.content == "final chunk"));
    };
    let projecting = async {
        session.cache.lock().reconcile_live(
            ChatMsg {
                id: "last".into(),
                kind: "assistant".into(),
                content: "final chunk".into(),
                ..Default::default()
            },
            1,
        );
        session.stream_progress.send_replace(7);
    };
    tokio::join!(biased;completing,projecting);
}

#[test]
fn silent_written_submit_releases_transport_gate_and_interrupts() {
    use crate::service::{
        delivery_error::DeliveryError,
        messenger::{AgentInput, HumanInput},
    };
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab, "opaque prompt", None)
        .unwrap();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let prompt: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(prompt["method"], "fixture/submit");
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["method"],
            "fixture/cancel"
        );
        peer.get_mut()
            .write_all(
                format!(
                    "{{\"id\":{},\"result\":{{\"stopReason\":\"cancelled\"}}}}\n",
                    prompt["id"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        peer
    });
    let sent = super::super::transport::send(
        &fixture.service,
        &tab,
        &AgentInput::Human(HumanInput::composer("opaque prompt")),
        Some(&pending.send_id),
    );
    assert!(matches!(sent, Err(DeliveryError::SubmitUnconfirmed { .. })));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&pending.send_id)
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
    assert_eq!(
        session.context.lock().active_turn_id.as_deref(),
        Some("turn"),
        "written prompts are interruptible before any own update"
    );
    let context = session.context.lock().clone();
    let request_start = started_at(&session, &context, Some("turn")).unwrap();
    assert_eq!(
        session.state.lock().active_turn_started_at_ms,
        Some(request_start)
    );
    assert!(super::super::controls::interrupt(&fixture.service, &tab).unwrap());
    assert!(session.context.lock().active_turn_id.is_none());
    assert!(session.completions.lock().contains("own-thread", "turn"));
    assert_eq!(
        session.context.lock().thread_id.as_deref(),
        Some("own-thread")
    );
    let _peer = crate::async_runtime::block_on(server).unwrap();
}

#[tokio::test]
async fn refused_before_write_never_publishes_a_turn_failure() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let context = session.context.lock().clone();
    let operation = operation(&context);
    let plan:RuntimePlan=serde_json::from_value(json!({"action":{"kind":"request","call":{"method":"fixture/submit","params":{},"target":{"threadId":"own-thread","turnId":"turn"}}},"state":null})).unwrap();
    let guard: rpc::PrewriteGuard =
        Arc::new(|| Err(RuntimeFailure::Refused("prewrite guard".into())));
    let result = run(
        &fixture.service,
        &session,
        &process,
        context,
        operation,
        plan,
        Some(guard),
    )
    .await;
    assert!(matches!(result, Err(RuntimeFailure::PrewriteRefused(_))));
    let mut changes = session.control_changes.subscribe();
    loop {
        changes.borrow_and_update();
        if session.turn_requests.lock().is_empty() {
            break;
        }
        changes.changed().await.unwrap();
    }
    assert!(session.cache.lock().snapshot().is_empty());
    assert_eq!(session.completions.lock().sequence(), 0);
}

#[test]
fn written_uncertain_send_is_confirmed_by_late_own_evidence_without_replay() {
    use crate::service::{
        chat_send_transaction::ChatSendState,
        delivery_error::DeliveryError,
        messenger::{AgentInput, HumanInput},
    };
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab, "opaque prompt", None)
        .unwrap();
    let (release, confirmed) = tokio::sync::oneshot::channel();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        confirmed.await.unwrap();
        let reply = json!({"id":request["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        peer
    });
    let sent = super::super::transport::send(
        &fixture.service,
        &tab,
        &AgentInput::Human(HumanInput::composer("opaque prompt")),
        Some(&pending.send_id),
    );
    assert!(matches!(sent, Err(DeliveryError::SubmitUnconfirmed { .. })));
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&pending.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    let context = session.context.lock().clone();
    crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![
            AgentRuntimeEvent::ChatItem {
                thread_id: "own-thread".into(),
                item: ChatMsg {
                    id: "late-user".into(),
                    kind: "user".into(),
                    content: "opaque prompt".into(),
                    ..Default::default()
                },
            },
            AgentRuntimeEvent::SendEvidence {
                thread_id: "own-thread".into(),
                send_id: pending.send_id.clone(),
                turn_id: "turn".into(),
                item_id: Some("late-user".into()),
            },
        ],
        1,
    ))
    .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&pending.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    release.send(()).unwrap();
    crate::async_runtime::block_on(completed(&session));
    let _peer = crate::async_runtime::block_on(server).unwrap();
}

#[test]
fn silent_written_reply_confirms_only_success_and_clears_its_claim() {
    use crate::service::{
        chat_send_transaction::ChatSendState,
        delivery_error::DeliveryError,
        messenger::{AgentInput, HumanInput},
    };
    for success in [true, false] {
        let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
        let session = engine::for_tab(&tab).unwrap();
        let pending = fixture
            .service
            .chat_send_ledger
            .try_start(&tab, "opaque prompt", None)
            .unwrap();
        let (release, ready) = tokio::sync::oneshot::channel();
        let server = crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            ready.await.unwrap();
            let reply = if success {
                json!({"id":request["id"],"result":{"stopReason":"end_turn"}})
            } else {
                json!({"id":request["id"],"error":{"code":-32603,"message":"opaque refusal"}})
            };
            peer.get_mut()
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            peer
        });
        let sent = super::super::transport::send(
            &fixture.service,
            &tab,
            &AgentInput::Human(HumanInput::composer("opaque prompt")),
            Some(&pending.send_id),
        );
        let Err(DeliveryError::SubmitUnconfirmed { detail, .. }) = sent else {
            panic!("silent written submit must remain unconfirmed until its reply");
        };
        assert!(!detail.contains("WrittenAwaitingEvidence"));
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&pending.send_id)
                .unwrap()
                .state,
            ChatSendState::Unknown
        );
        release.send(()).unwrap();
        crate::async_runtime::block_on(completed(&session));
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&pending.send_id)
                .unwrap()
                .state,
            if success {
                ChatSendState::Sent
            } else {
                ChatSendState::Unknown
            }
        );
        assert_eq!(
            session
                .context
                .lock()
                .claimed_sends
                .iter()
                .any(|claim| claim.send_id == pending.send_id),
            !success
        );
        assert!(session.context.lock().active_turn_id.is_none());
        if !success {
            assert!(session
                .cache
                .lock()
                .snapshot()
                .iter()
                .any(|row| row.content.contains("completion reply failed")));
        }
        let _peer = crate::async_runtime::block_on(server).unwrap();
    }
}

#[tokio::test]
async fn queued_written_prompt_starts_after_active_turn_settles_without_updates() {
    let source = SOURCE.replace("turnId:'turn'", "turnId:op.sendId");
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let (release_a, wait_a) = tokio::sync::oneshot::channel::<()>();
    let (release_b, wait_b) = tokio::sync::oneshot::channel::<()>();
    let server = async move {
        let mut peer = BufReader::new(peer);
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            requests.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        for (request, release) in requests.iter().zip([wait_a, wait_b]) {
            release.await.unwrap();
            let reply = json!({"id":request["id"],"result":{"stopReason":"end_turn"}});
            peer.get_mut()
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
        }
        peer
    };
    let exercise = async {
        for send in ["a", "b"] {
            let context = session.context.lock().clone();
            let mut op = operation(&context);
            if let RuntimeOperation::SubmitTurn {
                send_id,
                active_turn_id,
                ..
            } = &mut op
            {
                *send_id = send.into();
                *active_turn_id = context.active_turn_id.clone();
            }
            let (result, written) =
                engine::run_submit(&fixture.service, &session, &process, op).await;
            assert!(written);
            assert!(matches!(
                result,
                Err(RuntimeFailure::WrittenAwaitingEvidence)
            ));
            assert_eq!(session.context.lock().active_turn_id.as_deref(), Some("a"));
        }
        assert!(
            session.turn_requests.lock()["b"].activity.get().is_none(),
            "deferred activity does not consume the OnceCell"
        );
        let mut changes = session.control_changes.subscribe();
        release_a.send(()).unwrap();
        loop {
            changes.borrow_and_update();
            if session.context.lock().active_turn_id.as_deref() == Some("b") {
                break;
            }
            changes.changed().await.unwrap();
        }
        assert!(session.completions.lock().contains("own-thread", "a"));
        release_b.send(()).unwrap();
        loop {
            changes.borrow_and_update();
            if session.completions.lock().contains("own-thread", "b") {
                break;
            }
            changes.changed().await.unwrap();
        }
        assert!(session.context.lock().active_turn_id.is_none());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        let ((), _peer) = tokio::join!(exercise, server);
    })
    .await
    .expect("queued written activity must follow active-turn settlement");
}

#[tokio::test]
async fn concurrent_rpc_refusal_preserves_active_turn_and_late_delivery_wins() {
    use crate::service::chat_send_transaction::{ChatSendState, SendLiveBinding};
    use crate::service::pending_delivery_payloads::user_sends::{
        continuity::ReceiptState, UserSendAuthority,
    };
    let source = SOURCE
        .replace("turnId:'turn'", "turnId:op.sendId")
        .replace("beforeAcceptance:false", "beforeAcceptance:true");
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let server = async move {
        let mut peer = BufReader::new(peer);
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            requests.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        let first = &requests[0];
        let second = &requests[1];
        let reply = json!({"id":second["id"],"error":{"code":-32602,"message":"opaque"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        wait.await.unwrap();
        let reply = json!({"id":first["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    let exercise = async {
        let mut first = operation(&session.context.lock());
        if let RuntimeOperation::SubmitTurn { send_id, .. } = &mut first {
            *send_id = "a".into();
        }
        let (_, written) = engine::run_submit(&fixture.service, &session, &process, first).await;
        assert!(written);
        let context = session.context.lock().clone();
        let store = &fixture.service.pending_delivery_payloads.user_sends;
        let ledger = &fixture.service.chat_send_ledger;
        let reservation = store
            .reserve(
                UserSendAuthority {
                    tab_id: tab.clone(),
                    pty_id: None,
                    binding_nonce: context.binding_nonce.clone(),
                    pty_session_generation: None,
                    provider_id: "fixture".into(),
                    session_id: context.thread_id.clone(),
                    observed_session_id: context.thread_id.clone(),
                    prelaunch: false,
                },
                None,
                || ledger.reserve_identity(),
            )
            .unwrap();
        let record = store.admit(&reservation, "opaque", "opaque", None).unwrap();
        drop(reservation);
        ledger.admit_user(&record);
        assert!(ledger.bind_rpc_send(
            &record.send_id,
            SendLiveBinding {
                session_id: context.thread_id.clone(),
                binding_nonce: Some(context.binding_nonce.clone()),
            }
        ));
        assert!(store.claim(&record, context.thread_id.as_deref()).unwrap());
        session.context.lock().claimed_sends.push(ClaimedSend {
            send_id: record.send_id.clone(),
            thread_id: context.thread_id.clone().unwrap(),
            input: TurnInput {
                text: "opaque".into(),
                attachments: vec![],
            },
        });
        let mut second = operation(&context);
        if let RuntimeOperation::SubmitTurn {
            send_id,
            active_turn_id,
            ..
        } = &mut second
        {
            *send_id = record.send_id.clone();
            *active_turn_id = context.active_turn_id.clone();
        }
        let mut changes = session.control_changes.subscribe();
        let (_, written) = engine::run_submit(&fixture.service, &session, &process, second).await;
        assert!(written);
        loop {
            changes.borrow_and_update();
            if ledger.status(&record.send_id).unwrap().state == ChatSendState::Failed {
                break;
            }
            changes.changed().await.unwrap();
        }
        assert_eq!(session.context.lock().active_turn_id.as_deref(), Some("a"));
        assert_eq!(
            store.receipts(&tab).unwrap()[0].state,
            ReceiptState::RpcRefused
        );
        store.finish_handoff(&record, false).unwrap();
        store.complete(&record).unwrap();
        subscriber::publish_events(
            &fixture.service,
            &session,
            context.epoch,
            vec![AgentRuntimeEvent::SendEvidence {
                thread_id: context.thread_id.clone().unwrap(),
                send_id: record.send_id.clone(),
                turn_id: record.send_id.clone(),
                item_id: None,
            }],
            now(),
        )
        .await
        .unwrap();
        assert_eq!(
            ledger.status(&record.send_id).unwrap().state,
            ChatSendState::Sent
        );
        assert_eq!(
            store.receipts(&tab).unwrap()[0].state,
            ReceiptState::Delivered
        );
        assert_eq!(session.context.lock().active_turn_id.as_deref(), Some("a"));
        release.send(()).unwrap();
        loop {
            changes.borrow_and_update();
            if session.completions.lock().contains("own-thread", "a") {
                break;
            }
            changes.changed().await.unwrap();
        }
        assert!(session.context.lock().active_turn_id.is_none());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(exercise, server);
    })
    .await
    .expect("correlated refusal must settle independently without replay");
}

#[tokio::test]
async fn refused_claim_survives_its_own_idle_and_expires_at_next_other_settlement() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let context = session.context.lock().clone();
    let thread = context.thread_id.clone().unwrap();
    for send in ["refused", "unrelated"] {
        session.context.lock().claimed_sends.push(ClaimedSend {
            send_id: send.into(),
            thread_id: thread.clone(),
            input: TurnInput {
                text: format!("opaque-{send}"),
                attachments: vec![],
            },
        });
    }
    session.maps.lock().ensure_epoch(context.epoch);
    session
        .maps
        .lock()
        .remember_refused_claim(context.epoch, "refused", "refused-turn");
    let transition = |turn: &str, state| AgentRuntimeEvent::TurnState {
        thread_id: thread.clone(),
        turn_id: Some(turn.into()),
        parent_thread_id: None,
        state,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
    };
    subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![
            transition("refused-turn", RuntimeTurnState::Working),
            transition("refused-turn", RuntimeTurnState::Idle),
        ],
        now(),
    )
    .await
    .unwrap();
    assert!(session
        .context
        .lock()
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "refused"));
    subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![
            transition("next-turn", RuntimeTurnState::Working),
            transition("next-turn", RuntimeTurnState::Idle),
        ],
        now(),
    )
    .await
    .unwrap();
    let current = session.context.lock();
    assert!(!current
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "refused"));
    assert!(current
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "unrelated"));
}
