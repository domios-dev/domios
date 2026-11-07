use super::*;
use crate::service::{chat_send_transaction::ChatSendState, parked_user_delivery::UserDrain};

#[test]
fn a_user_send_during_an_active_turn_scoped_prompt_is_held_then_written_once_when_it_settles() {
    let source = SOURCE.replace("turnId:'turn'", "turnId:op.sendId").replace(
        "params:{threadId:op.threadId}",
        "params:{threadId:op.threadId,sendId:op.sendId}",
    );
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let svc = &fixture.service;
    svc.bind_user_send_owner();
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (second_written, second) = tokio::sync::oneshot::channel::<Value>();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let first: Value = serde_json::from_str(&line).unwrap();
        released.await.unwrap();
        let reply = json!({"id":first["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        second_written
            .send(serde_json::from_str(&line).unwrap())
            .unwrap();
        (peer, first)
    });
    let mut busy = operation(&session.context.lock());
    if let RuntimeOperation::SubmitTurn { send_id, .. } = &mut busy {
        *send_id = "busy".into();
    }
    let (_, written) =
        crate::async_runtime::block_on(engine::run_submit(svc, &session, &process, busy));
    assert!(written);

    let store = &svc.pending_delivery_payloads.user_sends;
    let reservation = store
        .reserve(svc.user_send_authority(&tab).unwrap(), None, || {
            svc.chat_send_ledger.reserve_identity()
        })
        .unwrap();
    let record = store
        .admit(
            &reservation,
            "opaque held prompt",
            "opaque held prompt",
            None,
        )
        .unwrap();
    drop(reservation);
    svc.chat_send_ledger.admit_user(&record);

    // The busy turn's prompt is still outstanding: the send is held before any write.
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    let held = svc.chat_send_ledger.status(&record.send_id).unwrap();
    assert_eq!(held.state, ChatSendState::Pending);
    assert_eq!(held.pending_reason.as_deref(), Some(ACTIVE_TURN_HOLD));
    assert!(!session.turn_requests.lock().contains_key(&record.send_id));
    assert!(!svc.chat_send_ledger.has_rpc_handoff(&record.send_id));
    assert_eq!(store.queued(&tab).unwrap().len(), 1);

    // The busy turn settles; the owner submits the held send exactly once.
    release.send(()).unwrap();
    let second = crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), second).await
    })
    .expect("a held send is written once its blocking turn settles")
    .unwrap();
    assert_eq!(second["method"], "fixture/submit");
    assert_eq!(second["params"]["sendId"], record.send_id.as_str());

    // Own-session evidence confirms it as today.
    let (thread, epoch) = {
        let context = session.context.lock();
        (context.thread_id.clone().unwrap(), context.epoch)
    };
    crate::async_runtime::block_on(subscriber::publish_events(
        svc,
        &session,
        epoch,
        vec![AgentRuntimeEvent::SendEvidence {
            thread_id: thread,
            send_id: record.send_id.clone(),
            turn_id: record.send_id.clone(),
            item_id: None,
        }],
        now(),
    ))
    .unwrap();
    assert_eq!(
        svc.chat_send_ledger.status(&record.send_id).unwrap().state,
        ChatSendState::Sent
    );
    let (mut peer, first) = crate::async_runtime::block_on(server).unwrap();
    assert_eq!(first["params"]["sendId"], "busy");
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufRead;
    assert!(
        matches!(
            Pin::new(&mut peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ),
        "the held prompt is written exactly once"
    );
}
