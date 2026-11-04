use super::super::engine::test_support::runtime_fixture_turn_scoped;
use super::*;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::json;
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
    return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'turn'},events:[{kind:'turnState',threadId:op.threadId,turnId:'turn',state:'idle'}]};
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
            })
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
        assert!(result.is_ok());
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

#[test]
fn prewrite_ingress_cannot_confirm_a_new_turn() {
    let (_fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let context = session.context.lock().clone();
    let pending = Arc::new(PendingTurn {
        context: context.clone(),
        send_id: "send".into(),
        settled: AtomicBool::new(false),
        turn: "turn".into(),
        generation: Arc::new(AtomicU64::new(2)),
        outcome: watch::channel(None).0,
        cancelled: watch::channel(false).0,
    });
    session.turn_requests.lock().insert("send".into(), pending);
    let frame = RuntimeFrame::Notification {
        method: "session/update".into(),
        params: json!({"sessionId":context.thread_id,"update":{"sessionUpdate":"agent_message_chunk"}}),
    };
    assert!(before_write(&session, 1, &frame));
    assert!(!before_write(&session, 2, &frame));
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
        turn: "turn".into(),
        generation: Arc::new(AtomicU64::new(1)),
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
