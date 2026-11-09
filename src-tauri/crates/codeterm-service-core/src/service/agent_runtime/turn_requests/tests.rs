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
        let recorded = session.completions.lock().contains(&thread, "turn");
        let pending = session.turn_requests.lock().values().any(|request| {
            request.context.thread_id.as_deref() == Some(thread.as_str()) && request.turn == "turn"
        });
        // The retirement marker blocks stale updates before Idle finishes projecting and persisting.
        if recorded && !pending {
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
        written_input: tokio::sync::OnceCell::new(),
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
        written_input: tokio::sync::OnceCell::new(),
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
    assert!(session.context.lock().active_turns.is_empty());
    assert!(session.completions.lock().contains("own-thread", "turn"));
    assert!(session.gate.try_acquire().is_ok());
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
    written_send_echo(false);
}

#[test]
fn image_send_keeps_one_metadata_only_user_row_after_echo() {
    written_send_echo(true);
}

fn written_send_echo(image: bool) {
    use crate::service::{
        chat_send_transaction::ChatSendState,
        delivery_error::DeliveryError,
        messenger::{AgentInput, HumanInput},
    };
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let image_dir = tempfile::tempdir().unwrap();
    let path = image_dir.path().join("image.png");
    std::fs::write(&path, [137, 80, 78, 71]).unwrap();
    let text = if image {
        format!(
            "inspect <domios type=\"attachment\" kind=\"image\" path=\"{}\" />",
            path.to_string_lossy()
        )
    } else {
        "opaque prompt".into()
    };
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab, &text, None)
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
        &AgentInput::Human(HumanInput::composer(&text)),
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
    let early = session.cache.lock().snapshot();
    assert_eq!(early.len(), 1);
    assert_eq!(early[0].content, text);
    assert_eq!(early[0].id, pending.send_id);
    if image {
        let attachments = early[0].attachments.as_ref().unwrap();
        assert_eq!(attachments.len(), 1);
        assert!(attachments[0].data.is_none());
        assert_eq!(attachments[0].path.as_deref(), path.to_str());
    }
    let original_id = early[0].id.clone();
    let context = session.context.lock().clone();
    let echoed_attachments = context
        .claimed_sends
        .iter()
        .find(|claim| claim.send_id == pending.send_id)
        .unwrap()
        .input
        .attachments
        .clone();
    if image {
        assert!(echoed_attachments[0].data.is_some());
    }
    crate::async_runtime::block_on(subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![AgentRuntimeEvent::SendEvidence {
            thread_id: "own-thread".into(),
            send_id: pending.send_id.clone(),
            turn_id: "turn".into(),
            item_id: Some(original_id.clone()),
        }],
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
        ChatSendState::Unknown
    );
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
                    content: text.clone(),
                    attachments: (!echoed_attachments.is_empty()).then_some(echoed_attachments),
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
    let users = session.cache.lock().snapshot();
    assert_eq!(users.iter().filter(|row| row.kind == "user").count(), 1);
    if image {
        let row = users.iter().find(|row| row.kind == "user").unwrap();
        assert!(row.attachments.as_ref().unwrap()[0].data.is_none());
        assert_eq!(row.runtime_source_item_id.as_deref(), Some("late-user"));
    }
    assert_eq!(
        users.iter().find(|row| row.kind == "user").unwrap().id,
        original_id
    );
    let _peer = crate::async_runtime::block_on(server).unwrap();
}

#[test]
fn silent_written_reply_without_session_evidence_keeps_its_claim() {
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
        crate::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), completed(&session))
                .await
                .expect("silent reply projection and request retirement must settle");
        });
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&pending.send_id)
                .unwrap()
                .state,
            ChatSendState::Unknown
        );
        assert_eq!(
            session
                .context
                .lock()
                .claimed_sends
                .iter()
                .any(|claim| claim.send_id == pending.send_id),
            true
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
async fn silent_reply_settlement_follows_the_retirement_marker() {
    use std::{future::Future, task::Poll};
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let op = operation(&session.context.lock());
    let (release, written) = tokio::sync::oneshot::channel();
    let exercise = async {
        let (result, handed_off) =
            engine::run_submit(&fixture.service, &session, &process, op).await;
        assert!(handed_off);
        assert!(matches!(
            result,
            Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
        engine::record_completed_turn(&session, "own-thread", Some("turn"));
        assert!(session.completions.lock().contains("own-thread", "turn"));
        assert_eq!(
            session.context.lock().active_turn_id.as_deref(),
            Some("turn")
        );
        let mut settled = Box::pin(completed(&session));
        let pending =
            std::future::poll_fn(|cx| Poll::Ready(settled.as_mut().poll(cx).is_pending())).await;
        assert!(
            pending,
            "a retirement marker alone is not a completed request"
        );
        release.send(()).unwrap();
        settled.await;
        assert!(session.context.lock().active_turn_id.is_none());
        assert!(session.turn_requests.lock().is_empty());
    };
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        written.await.unwrap();
        let reply = json!({"id":request["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(exercise, server);
    })
    .await
    .expect("settlement waits must follow request retirement without sleeps");
}

mod active_turn_hold;
mod busy_refusal;
#[path = "tests/cancelled_delivery.rs"]
mod cancelled_delivery;
mod early_input;

#[tokio::test]
async fn provider_refusal_before_handoff_is_a_prewrite_failure() {
    use crate::service::{
        delivery_error::{DeliveryError, SendOrigin},
        messenger::{AgentInput, HumanInput},
    };
    const REFUSAL: &str = r#"module.exports = {
        runtimePlan() { return {action:{kind:'refused',reason:'Command was not advertised by this session'},state:null}; },
        runtimeResponse() { throw new Error('no request was written'); },
        runtimeReduce() { return {state:null,events:[]}; }
    };"#;
    let (fixture, tab, mut peer, _cleanup) = runtime_fixture_turn_scoped(REFUSAL);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&tab, "/unadvertised opaque", None)
        .unwrap();
    let result = super::super::transport::submit_loaded(
        &fixture.service,
        &session,
        &process,
        &pending.send_id,
        AgentInput::Human(HumanInput::composer("/unadvertised opaque")),
    )
    .await;
    let error = result.unwrap_err();
    assert!(
        matches!(&error, DeliveryError::TabBusyRefused { detail,.. } if detail.starts_with("runtime_rejected_before_write:"))
    );
    assert!(!error.bytes_written());
    assert!(error.surfaces_as_failure(SendOrigin::Programmatic));
    assert!(!fixture
        .service
        .chat_send_ledger
        .has_rpc_handoff(&pending.send_id));
    assert!(!session
        .context
        .lock()
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == pending.send_id));
    assert!(session.context.lock().active_turn_id.is_none());
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::{AsyncRead, ReadBuf};
    let mut buffer = [0; 1];
    assert!(matches!(
        Pin::new(&mut peer).poll_read(
            &mut Context::from_waker(Waker::noop()),
            &mut ReadBuf::new(&mut buffer)
        ),
        Poll::Pending
    ));
}
