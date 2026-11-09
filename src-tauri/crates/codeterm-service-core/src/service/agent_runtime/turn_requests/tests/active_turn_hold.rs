use super::*;
use crate::service::{chat_send_transaction::ChatSendState, parked_user_delivery::UserDrain};

fn continue_after_restart(
    svc: &ServiceContext,
    tab: &str,
    session: &Session,
) -> Result<crate::service::messenger::Delivery, String> {
    let (thread, nonce) = {
        let context = session.context.lock();
        (
            context.thread_id.clone().unwrap(),
            context.binding_nonce.clone(),
        )
    };
    crate::service::messenger::Messenger::deliver_turn_resume(
        svc,
        tab,
        None,
        &thread,
        &nonce,
        None,
        "opaque continuation",
    )
}

#[test]
fn a_restart_continue_written_to_a_turn_scoped_runtime_is_a_success_not_a_refusal() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    svc.bind_user_send_owner();
    let session = engine::for_tab(&tab).unwrap();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        (peer, line)
    });
    let delivered = continue_after_restart(svc, &tab, &session);
    assert!(
        delivered.is_ok(),
        "a written continue awaits evidence: {delivered:?}"
    );
    let (mut peer, line) = crate::async_runtime::block_on(server).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["method"],
        "fixture/submit"
    );
    assert!(svc
        .pending_delivery_payloads
        .user_sends
        .queued(&tab)
        .unwrap()
        .is_empty());
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
fn a_restart_continue_behind_a_running_turn_is_held_then_written_once() {
    use crate::service::messenger::Delivery;
    let source = SOURCE.replace("turnId:'turn'", "turnId:op.sendId");
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let svc = &fixture.service;
    svc.bind_user_send_owner();
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (second_written, second) = tokio::sync::oneshot::channel::<()>();
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
        second_written.send(()).unwrap();
        peer
    });
    let mut busy = operation(&session.context.lock());
    if let RuntimeOperation::SubmitTurn { send_id, .. } = &mut busy {
        *send_id = "busy".into();
    }
    let (_, written) =
        crate::async_runtime::block_on(engine::run_submit(svc, &session, &process, busy));
    assert!(written);

    let held = continue_after_restart(svc, &tab, &session).unwrap();
    let Delivery::Pending { delivery_id } = held else {
        panic!("a running turn holds the continue before any write");
    };
    assert_eq!(
        svc.chat_send_ledger
            .status(&delivery_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(ACTIVE_TURN_HOLD)
    );

    release.send(()).unwrap();
    crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), second).await
    })
    .expect("the held continue is written once its blocking turn settles")
    .unwrap();
    let mut peer = crate::async_runtime::block_on(server).unwrap();
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
fn a_runtime_that_stopped_does_not_read_as_a_failed_turn() {
    for reason in [
        RuntimeFailure::PipeEof,
        RuntimeFailure::Superseded,
        RuntimeFailure::ProcessExited {
            code: None,
            signal: Some(9),
            stderr: String::new(),
            requested: true,
        },
    ] {
        assert_eq!(
            reply_failure_detail(&reason),
            RuntimeTurnDetail::Interrupted
        );
    }
    assert_eq!(
        reply_failure_detail(&RuntimeFailure::Refused("opaque".into())),
        RuntimeTurnDetail::Failed
    );
}

#[tokio::test]
async fn ending_a_turn_for_a_lifecycle_change_never_claims_it_failed() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let operation = operation(&session.context.lock());
    let (_, written) = engine::run_submit(&fixture.service, &session, &process, operation).await;
    assert!(written);
    end_for_lifecycle(&fixture.service, &session, "opaque lifecycle end").await;
    assert!(session.context.lock().active_turn_id.is_none());
    let mux = fixture.service.mux.read();
    let live = mux.get_tab(&tab).unwrap().agent_state.clone().unwrap();
    let expected = crate::agent::state_verdict::live_state_for_runtime(
        &RuntimeTurnState::Idle,
        Some(RuntimeTurnDetail::Interrupted),
        None,
    );
    assert_eq!(live, expected);
}

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

fn source_with_send_ids() -> String {
    SOURCE.replace("turnId:'turn'", "turnId:op.sendId").replace(
        "params:{threadId:op.threadId}",
        "params:{threadId:op.threadId,sendId:op.sendId}",
    )
}

/// What automatic recovery publishes for a dead runtime just before it relaunches.
fn publish_recovery_degrade(svc: &ServiceContext, tab: &str, session: &Arc<Session>) {
    let epoch = session.state.lock().epoch;
    session
        .state
        .lock()
        .degrade(epoch, &RuntimeFailure::Refused("opaque exit".into()));
    engine::publish(svc, tab, session);
}

fn assert_still_held(svc: &ServiceContext, tab: &str, send_id: &str) {
    let held = svc.chat_send_ledger.status(send_id).unwrap();
    assert_eq!(held.state, ChatSendState::Pending);
    assert_eq!(held.error, None);
    assert!(!svc.chat_send_ledger.has_rpc_handoff(send_id));
    assert!(svc
        .pending_delivery_payloads
        .user_sends
        .queued(tab)
        .unwrap()
        .iter()
        .any(|queued| queued.send_id == send_id));
}

#[test]
fn a_transient_recovery_degrade_holds_the_send_and_the_relaunch_writes_it_once() {
    let source = source_with_send_ids();
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let record = held_behind_a_turn_with_no_prompt(svc, &tab, &session);
    let thread = session.context.lock().thread_id.clone().unwrap();
    let epoch = session.state.lock().epoch;
    let exit = svc.begin_runtime_exit(&tab);
    publish_recovery_degrade(svc, &tab, &session);
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    assert_still_held(svc, &tab, &record.send_id);

    assert!(session.state.lock().load(epoch, &thread));
    engine::publish(svc, &tab, &session);
    let mut peer = BufReader::new(peer);
    let mut line = String::new();
    crate::async_runtime::block_on(async {
        drop(exit);
        tokio::time::timeout(Duration::from_secs(5), peer.read_line(&mut line))
            .await
            .expect("recovery completing wakes the owner")
            .unwrap();
    });
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "fixture/submit");
    assert_eq!(request["params"]["sendId"], record.send_id.as_str());
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
        "the held send is written exactly once"
    );
}

#[test]
fn a_recovery_that_really_failed_settles_the_held_send_once_its_outcome_is_known() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let record = held_behind_a_turn_with_no_prompt(svc, &tab, &session);
    let exit = svc.begin_runtime_exit(&tab);
    publish_recovery_degrade(svc, &tab, &session);
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    assert_still_held(svc, &tab, &record.send_id);

    drop(exit);
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
    assert!(!svc.chat_send_ledger.has_rpc_handoff(&record.send_id));
}

#[test]
fn recovery_resumes_the_interrupted_turn_first_and_the_held_send_follows_it_once() {
    let source = source_with_send_ids();
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let record = held_behind_a_turn_with_no_prompt(svc, &tab, &session);
    let thread = session.context.lock().thread_id.clone().unwrap();
    let epoch = session.state.lock().epoch;
    let exit = svc.begin_runtime_exit(&tab);
    publish_recovery_degrade(svc, &tab, &session);
    assert!(session.state.lock().load(epoch, &thread));
    engine::publish(svc, &tab, &session);

    let (first_read, first) = tokio::sync::oneshot::channel::<Value>();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (second_read, second) = tokio::sync::oneshot::channel::<Value>();
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        first_read.send(request.clone()).unwrap();
        released.await.unwrap();
        let reply = json!({"id":request["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        second_read
            .send(serde_json::from_str(&line).unwrap())
            .unwrap();
        peer
    });

    continue_after_restart(svc, &tab, &session).unwrap();
    let first = crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), first).await
    })
    .expect("the continuation is written while recovery still owns the runtime")
    .unwrap();
    assert_ne!(first["params"]["sendId"], record.send_id.as_str());
    assert_still_held(svc, &tab, &record.send_id);

    drop(exit);
    assert!(matches!(
        crate::service::agent_runtime::user_delivery::drain(svc, &tab).unwrap(),
        UserDrain::Idle
    ));
    assert_still_held(svc, &tab, &record.send_id);
    assert_eq!(
        svc.chat_send_ledger
            .status(&record.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(ACTIVE_TURN_HOLD)
    );

    release.send(()).unwrap();
    let second = crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), second).await
    })
    .expect("the held send follows the resumed turn")
    .unwrap();
    assert_eq!(second["params"]["sendId"], record.send_id.as_str());
    let mut peer = crate::async_runtime::block_on(server).unwrap();
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
        "the held send is written exactly once"
    );
}
