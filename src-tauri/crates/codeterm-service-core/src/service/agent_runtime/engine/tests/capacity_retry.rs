use super::super::*;
use super::sync_send::runtime_fixture;
use crate::delayed_send::DelayedSendQueue;
use crate::service::messenger::HumanInput;
use serde_json::json;

fn fail(svc: &ServiceContext, session: &Arc<Session>, turn: &str) {
    let epoch = session.context.lock().epoch;
    crate::async_runtime::block_on(super::super::super::subscriber::publish_events(
        svc,
        session,
        epoch,
        vec![
            AgentRuntimeEvent::ChatItem {
                thread_id: "own-thread".into(),
                item: codeterm_types::agent::chat_types::ChatMsg {
                    id: format!("{turn}:error"),
                    kind: "system".into(),
                    content: "opaque diagnostic".into(),
                    system_subtype: Some("api_error".into()),
                    tool_flow_id: Some(turn.into()),
                    tool_input: Some(json!({"retryableKind":"transient","retryable":false})),
                    ..Default::default()
                },
            },
            AgentRuntimeEvent::TurnState {
                thread_id: "own-thread".into(),
                turn_id: Some(turn.into()),
                state: RuntimeTurnState::Idle,
                detail: Some(RuntimeTurnDetail::Failed),
                status_line: None,
                started_at: None,
                completed_at: None,
                parent_thread_id: None,
            },
        ],
        123,
    ))
    .unwrap();
}

#[test]
fn capacity_retry_user_send_and_interrupt_cancel_pending_and_reset_count() {
    for interrupt in [false, true] {
        let (fixture, tab, _peer, _cleanup) = runtime_fixture();
        let session = for_tab(&tab).unwrap();
        fail(&fixture.service, &session, "first");
        fail(&fixture.service, &session, "second");
        let queue = fixture.service.ext::<DelayedSendQueue>();
        assert_eq!(queue.list(Some(&tab)).len(), 1);
        if interrupt {
            super::super::super::controls::interrupt(&fixture.service, &tab).unwrap();
        } else {
            fixture
                .service
                .pending_delivery_payloads
                .user_sends
                .tab(&tab)
                .running
                .store(true, std::sync::atomic::Ordering::Release);
            fixture
                .service
                .start_human_chat_send(&tab, HumanInput::composer("new user turn"), None, None)
                .unwrap();
        }
        assert!(queue.claim_due(10_123).unwrap().is_empty());
        fail(&fixture.service, &session, "next");
        assert_eq!(
            session
                .cache
                .lock()
                .snapshot()
                .iter()
                .filter(|item| item.content == "Automatic retry: attempt 1/10")
                .count(),
            2
        );
    }
}

#[test]
fn capacity_retry_turn_started_after_claim_is_dropped_without_reset() {
    let (fixture, tab, mut peer, _cleanup) = runtime_fixture();
    let session = for_tab(&tab).unwrap();
    fail(&fixture.service, &session, "first");
    let queue = fixture.service.ext::<DelayedSendQueue>();
    let message = queue.claim_due(10_123).unwrap().pop().unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    session.context.lock().active_turn_id = Some("busy".into());
    let rows_before = session.cache.lock().snapshot().len();
    let result = crate::async_runtime::block_on(crate::delayed_send::deliver_claimed(
        &Arc::new(fixture.service.clone()),
        message,
    ));
    result.unwrap();
    assert!(fixture.service.chat_send_ledger.reconcile(&tab).is_empty());
    assert_no_submit(&mut peer);
    assert!(queue.list(Some(&tab)).is_empty());
    assert_eq!(session.cache.lock().snapshot().len(), rows_before);
    fail(&fixture.service, &session, "busy");
    assert!(session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.content == "Automatic retry: attempt 2/10"));
}

#[test]
fn capacity_retry_ordinary_continue_uses_normal_send_once_and_keeps_count() {
    assert_delayed_continue(false, false);
}

#[test]
fn capacity_retry_ordinary_delayed_continue_reaches_busy_runtime() {
    assert_delayed_continue(true, false);
}

#[test]
fn capacity_retry_cancel_after_claim_drops_retry_but_delivers_ordinary_continue() {
    assert_delayed_continue(false, true);
}

fn assert_no_submit(peer: &mut tokio::io::DuplexStream) {
    use std::future::Future;
    use std::task::{Context, Waker};
    use tokio::io::AsyncReadExt;
    let mut byte = [0];
    let mut read = std::pin::pin!(peer.read(&mut byte));
    assert!(read
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

fn assert_delayed_continue(busy: bool, cancel_after_claim: bool) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (fixture, tab, mut peer, _cleanup) = runtime_fixture();
    let session = for_tab(&tab).unwrap();
    fail(&fixture.service, &session, "first");
    let queue = fixture.service.ext::<DelayedSendQueue>();
    if cancel_after_claim {
        let claimed = queue.claim_due(10_123).unwrap().pop().unwrap();
        super::super::super::capacity_retry::cancel(&fixture.service, &tab).unwrap();
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab)
            .unwrap()
            .boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        crate::async_runtime::block_on(crate::delayed_send::deliver_claimed(
            &Arc::new(fixture.service.clone()),
            claimed,
        ))
        .unwrap();
        assert!(fixture.service.chat_send_ledger.reconcile(&tab).is_empty());
        assert_no_submit(&mut peer);
        assert!(queue.list(Some(&tab)).is_empty());
    }
    let message = if busy || cancel_after_claim {
        if busy {
            session.context.lock().active_turn_id = Some("busy".into());
        }
        queue
            .enqueue(
                tab.clone(),
                None,
                Some("own-thread".into()),
                "continue".into(),
                Some(10_123),
                Default::default(),
            )
            .unwrap()
    } else {
        queue.list(Some(&tab)).pop().unwrap()
    };
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    let responder = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "fixture/submit");
        let text = request["params"]["input"]["text"].as_str().unwrap();
        assert!(text.ends_with(">continue</domios>"));
        let response = json!({"id":request["id"],"result":{"turnId":"retry-turn"}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        (
            peer,
            request["params"]["sendId"].as_str().unwrap().to_owned(),
        )
    });
    crate::async_runtime::block_on(crate::delayed_send::send_message_now(
        Arc::new(fixture.service.clone()),
        message.id.clone(),
    ))
    .unwrap();
    let (_peer, send_id) = crate::async_runtime::block_on(responder).unwrap();
    assert_eq!(queue.list(Some(&tab)).len(), usize::from(busy));
    assert!(
        crate::async_runtime::block_on(crate::delayed_send::send_message_now(
            Arc::new(fixture.service.clone()),
            message.id,
        ))
        .is_err()
    );
    assert_eq!(fixture.service.chat_send_ledger.reconcile(&tab).len(), 1);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Written
    );
    fail(&fixture.service, &session, "retry-turn");
    let expected_attempt = if cancel_after_claim { 1 } else { 2 };
    assert!(session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.content == format!("Automatic retry: attempt {expected_attempt}/10")));
}
