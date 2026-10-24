use super::super::*;
use super::sync_send::{
    runtime_fixture, runtime_fixture_with_notifications, RUNTIME_FIXTURE_SOURCE,
};
use crate::delayed_send::DelayedSendQueue;
use crate::service::{
    agent_runtime::transport,
    messenger::{AgentInput, HumanInput},
};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn fail_capacity(svc: &ServiceContext, session: &Arc<Session>, turn: &str) {
    let epoch = session.context.lock().epoch;
    super::super::super::subscriber::publish_events(
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
                parent_thread_id: None,
                state: RuntimeTurnState::Idle,
                detail: Some(RuntimeTurnDetail::Failed),
                status_line: None,
                started_at: None,
                completed_at: None,
            },
        ],
        123,
    )
    .await
    .unwrap();
}

#[test]
fn capacity_retry_delayed_runtime_send_has_fresh_ledger_identity_and_same_payload() {
    let source = RUNTIME_FIXTURE_SOURCE.replace(
        "runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }",
        r#"runtimeReduce(ctx, state, frames) { return {state:null,events:frames.flatMap(frame=>[
            {kind:'chatItem',threadId:frame.params.threadId,item:{id:'capacity-error',type:'system',content:'opaque diagnostic',systemSubtype:'api_error',toolFlowId:frame.params.turnId,toolInput:{retryableKind:'transient',retryable:false}}},
            {kind:'turnState',threadId:frame.params.threadId,turnId:frame.params.turnId,state:'idle',detail:'failed'}
        ])}; }"#,
    );
    assert_ne!(source, RUNTIME_FIXTURE_SOURCE);
    let (fixture, tab, peer, _cleanup) =
        runtime_fixture_with_notifications(&source, &["fixture/capacity"]);
    let responder = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut sends = Vec::new();
        for turn in ["original-turn", "retry-turn"] {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "fixture/submit");
            assert_eq!(request["params"]["input"]["text"], "opaque user payload");
            sends.push(request["params"]["sendId"].as_str().unwrap().to_owned());
            let response = json!({"id":request["id"],"result":{"turnId":turn}});
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
        (peer, sends)
    });
    transport::send(
        &fixture.service,
        &tab,
        &AgentInput::Human(HumanInput::composer("opaque user payload")),
        None,
    )
    .unwrap();
    let session = for_tab(&tab).unwrap();
    let adapter = session.adapter.lock().clone().unwrap();
    let context = session.context.lock().clone();
    let reduction = crate::async_runtime::block_on(adapter.runtime_reduce(
        &context,
        &Value::Null,
        &[RuntimeFrame::Notification {
            method: "fixture/capacity".into(),
            params: json!({"threadId":"own-thread","turnId":"original-turn"}),
        }],
    ))
    .unwrap();
    crate::async_runtime::block_on(super::super::super::subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        reduction.events,
        123,
    ))
    .unwrap();
    let messages = fixture.service.ext::<DelayedSendQueue>().list(Some(&tab));
    assert_eq!(messages.len(), 1);
    let msg = &messages[0];
    assert_eq!(msg.turn_retry.as_ref().unwrap().attempt, 1);
    crate::async_runtime::block_on(crate::delayed_send::send_message_now(
        Arc::new(fixture.service.clone()),
        msg.id.clone(),
    ))
    .unwrap();
    let (_peer, sends) = crate::async_runtime::block_on(responder).unwrap();
    assert_ne!(sends[0], sends[1]);
    for (send, expected) in sends.into_iter().zip([
        crate::service::chat_send_transaction::ChatSendState::Written,
        crate::service::chat_send_transaction::ChatSendState::Written,
    ]) {
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&send)
                .unwrap()
                .state,
            expected
        );
    }
    assert!(fixture
        .service
        .ext::<DelayedSendQueue>()
        .list(Some(&tab))
        .is_empty());
    assert_eq!(
        session.context.lock().active_turn_id.as_deref(),
        Some("retry-turn")
    );
}

#[test]
fn capacity_retry_new_user_send_and_model_choice_cancel_the_queued_attempt() {
    for model_choice in [false, true] {
        let (fixture, tab, _peer, _cleanup) = runtime_fixture();
        let session = for_tab(&tab).unwrap();
        session
            .capacity_retry
            .lock()
            .begin("original", "opaque payload");
        session.capacity_retry.lock().bind("original", "turn");
        crate::async_runtime::block_on(fail_capacity(&fixture.service, &session, "turn"));
        assert_eq!(
            fixture
                .service
                .ext::<DelayedSendQueue>()
                .list(Some(&tab))
                .len(),
            1
        );
        let queue = fixture.service.ext::<DelayedSendQueue>();
        let current = queue.list(Some(&tab)).pop().unwrap();
        let old = queue
            .enqueue_turn_retry(
                &tab,
                "older turn",
                9_999,
                current.turn_retry.clone().unwrap(),
            )
            .unwrap();
        crate::delayed_send::delete_message(&fixture.service, old.id).unwrap();
        assert_eq!(
            session.capacity_retry.lock().pending.as_deref(),
            Some(current.id.as_str())
        );
        assert_eq!(queue.list(Some(&tab)).len(), 1);
        if model_choice {
            assert!(super::super::super::controls::switch(
                &fixture.service,
                &tab,
                "unknown-model",
                None,
                None,
                None
            )
            .is_err());
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
        assert!(fixture
            .service
            .ext::<DelayedSendQueue>()
            .claim_due(10_123)
            .unwrap()
            .is_empty());
        assert!(session.capacity_retry.lock().pending.is_none());
    }
}
