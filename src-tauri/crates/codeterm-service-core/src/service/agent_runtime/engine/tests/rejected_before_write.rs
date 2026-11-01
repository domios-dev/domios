use super::super::*;
use super::sync_send::{runtime_fixture_with, RUNTIME_FIXTURE_SOURCE};
use crate::service::{chat_send_transaction::ChatSendState, test_fixture::ServiceFixture};
use std::sync::atomic::Ordering;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn admit_user_sends(fixture: &ServiceFixture, tab_id: &str, texts: &[&str]) -> Vec<String> {
    let session = for_tab(tab_id).unwrap();
    fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .tab(tab_id)
        .running
        .store(true, Ordering::Release);
    bind(&fixture.service, &session, "own-thread", false).unwrap();
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            fixture
                .service
                .start_human_chat_send(
                    tab_id,
                    crate::service::messenger::HumanInput::composer(*text),
                    None,
                    Some(format!("opaque-token-{index}")),
                )
                .unwrap()
                .send_id
        })
        .collect()
}

fn assert_nothing_written(peer: &mut tokio::io::DuplexStream) {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncRead;
    let mut bytes = [0; 1];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    assert!(matches!(
        Pin::new(peer).poll_read(&mut Context::from_waker(Waker::noop()), &mut buffer),
        Poll::Pending
    ));
}

#[test]
fn provider_hook_exception_before_handoff_fails_each_user_send_without_writing() {
    let source = RUNTIME_FIXTURE_SOURCE.replace(
        "runtimePlan(ctx, op) {",
        "runtimePlan(ctx, op) {\n    if (op.kind === 'submitTurn') throw new Error('opaque');",
    );
    let (fixture, tab_id, mut peer, _cleanup) = runtime_fixture_with(&source);
    let sends = admit_user_sends(
        &fixture,
        &tab_id,
        &["first opaque turn", "second opaque turn"],
    );

    fixture.service.drain_user_owner(&tab_id).unwrap();

    assert_nothing_written(&mut peer);
    for id in &sends {
        let status = fixture.service.chat_send_ledger.status(id).unwrap();
        assert_eq!(status.state, ChatSendState::Failed);
        assert!(status.error.is_some());
        assert!(!fixture
            .service
            .chat_send_ledger
            .user_payload_may_have_been_written(id));
    }
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .queued(&tab_id)
        .unwrap()
        .is_empty());
}

#[test]
fn provider_hook_exception_after_handoff_stays_uncertain_and_is_never_replayed() {
    let source = RUNTIME_FIXTURE_SOURCE.replace(
        "runtimeResponse(ctx, op, state, reply) {",
        "runtimeResponse(ctx, op, state, reply) {\n    if (op.kind === 'submitTurn') throw new Error('opaque');",
    );
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture_with(&source);
    let sends = admit_user_sends(&fixture, &tab_id, &["opaque turn"]);
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "fixture/submit");
        let response = serde_json::json!({"id":request["id"],"result":{"turnId":"opaque-turn"}});
        peer.get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        peer
    });

    fixture.service.drain_user_owner(&tab_id).unwrap();

    let mut peer = crate::async_runtime::block_on(peer).unwrap().into_inner();
    let status = fixture.service.chat_send_ledger.status(&sends[0]).unwrap();
    assert_eq!(status.state, ChatSendState::Unknown);
    fixture.service.drain_user_owner(&tab_id).unwrap();
    assert_nothing_written(&mut peer);
}
