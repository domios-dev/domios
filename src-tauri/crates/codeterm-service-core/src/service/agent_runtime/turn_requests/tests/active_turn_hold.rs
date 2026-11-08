use super::*;
use crate::service::{chat_send_transaction::ChatSendState, parked_user_delivery::UserDrain};

fn held_behind_a_turn_with_no_prompt(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
) -> crate::service::pending_delivery_payloads::user_sends::UserSendRecord {
    svc.bind_user_send_owner();
    let (thread, epoch) = {
        let context = session.context.lock();
        (context.thread_id.clone().unwrap(), context.epoch)
    };
    crate::async_runtime::block_on(subscriber::publish_events(
        svc,
        session,
        epoch,
        vec![AgentRuntimeEvent::TurnState {
            thread_id: thread,
            turn_id: Some("no-prompt-behind-this".into()),
            parent_thread_id: None,
            state: RuntimeTurnState::Working,
            detail: None,
            status_line: None,
            started_at: None,
            completed_at: None,
        }],
        now(),
    ))
    .unwrap();
    let store = &svc.pending_delivery_payloads.user_sends;
    let reservation = store
        .reserve(svc.user_send_authority(tab).unwrap(), None, || {
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
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, tab).unwrap(),
        UserDrain::Idle
    ));
    assert_eq!(
        svc.chat_send_ledger
            .status(&record.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(ACTIVE_TURN_HOLD)
    );
    record
}

fn assert_turn_released(svc: &ServiceContext, tab: &str, session: &Session) {
    assert!(session.context.lock().active_turn_id.is_none());
    assert!(session.state.lock().active_turn_started_at_ms.is_none());
    let mux = svc.mux.read();
    let target = mux.get_tab(tab).unwrap();
    assert_eq!(
        target.agent_state.as_ref().unwrap().state,
        codeterm_types::pty::TimelineState::Idle
    );
    assert!(target.agent_runtime_state.as_ref().unwrap()["active_turn_started_at_ms"].is_null());
}

#[test]
fn a_degrade_recovery_cannot_lift_clears_the_turn_and_shows_a_held_send_as_not_delivered() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let record = held_behind_a_turn_with_no_prompt(svc, &tab, &session);
    let epoch = session.state.lock().epoch;
    session
        .state
        .lock()
        .degrade(epoch, &RuntimeFailure::Refused("opaque refusal".into()));
    engine::publish_degraded(svc, &tab, &session);
    assert_turn_released(svc, &tab, &session);
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    let settled = svc.chat_send_ledger.status(&record.send_id).unwrap();
    assert_eq!(settled.state, ChatSendState::Failed);
    assert_eq!(
        settled.error.as_deref(),
        Some("launch_degraded_before_write")
    );
    assert!(svc
        .pending_delivery_payloads
        .user_sends
        .queued(&tab)
        .unwrap()
        .is_empty());
    assert!(!svc.chat_send_ledger.has_rpc_handoff(&record.send_id));
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufRead;
    let mut peer = BufReader::new(peer);
    assert!(matches!(
        Pin::new(&mut peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[test]
fn a_held_send_is_written_once_when_a_degraded_runtime_is_ready_again() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let record = held_behind_a_turn_with_no_prompt(svc, &tab, &session);
    let thread = session.context.lock().thread_id.clone().unwrap();
    let epoch = session.state.lock().epoch;
    session
        .state
        .lock()
        .degrade(epoch, &RuntimeFailure::Deadline);
    engine::publish(svc, &tab, &session);
    assert_turn_released(svc, &tab, &session);
    assert!(session.state.lock().load(epoch, &thread));
    engine::publish(svc, &tab, &session);
    let mut peer = BufReader::new(peer);
    let mut line = String::new();
    crate::async_runtime::block_on(async {
        let written = tokio::time::timeout(Duration::from_secs(5), peer.read_line(&mut line));
        let drained = tokio::task::spawn_blocking({
            let (svc, tab) = (svc.clone(), tab.clone());
            move || crate::service::agent_runtime::user_delivery::drain(&svc, &tab).map(|_| ())
        });
        written.await.unwrap().unwrap();
        drop(drained);
    });
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "fixture/submit");
    assert!(svc.chat_send_ledger.has_rpc_handoff(&record.send_id));
    assert!(svc
        .pending_delivery_payloads
        .user_sends
        .queued(&tab)
        .unwrap()
        .iter()
        .all(|queued| queued.send_id != record.send_id));
}

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
