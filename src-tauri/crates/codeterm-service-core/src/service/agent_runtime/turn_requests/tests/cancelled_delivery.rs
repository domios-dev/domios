use super::*;
use crate::service::{
    delivery_error::DeliveryError,
    messenger::{AgentInput, HumanInput, MessageEnvelope, Messenger},
};

#[test]
fn written_turn_end_before_output_keeps_one_user_row_and_next_turn_order() {
    for cancelled in [true, false] {
        let source = SOURCE.replace("turnId:'turn'","turnId:op.sendId").replace(
        "events:[{kind:'turnState'",
        "events:[{kind:'chatItem',threadId:op.threadId,item:{id:'user-'+op.sendId,seq:op.input.text==='opaque next prompt' ? 2 : 1,type:'user',content:op.input.text}},{kind:'turnState'",
    );
        let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
        let session = engine::for_tab(&tab).unwrap();
        let frame =
            Messenger::from(MessageEnvelope::default()).framed_delivery("opaque cancelled prompt");
        assert!(frame.to_message().starts_with("<domios from=\"cli\""));
        assert_eq!(frame.body(), "opaque cancelled prompt");
        let wire = frame.to_message();
        let first = fixture
            .service
            .chat_send_ledger
            .try_start(&tab, &frame.to_message(), None)
            .unwrap();
        let expected = frame.chat_message(first.send_id.clone()).unwrap();
        let (release, written) = tokio::sync::oneshot::channel();
        let server = crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            let first: Value = serde_json::from_str(&line).unwrap();
            let response = if cancelled {
                line.clear();
                peer.read_line(&mut line).await.unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&line).unwrap()["method"],
                    "fixture/cancel"
                );
                json!({"id":first["id"],"result":{"stopReason":"cancelled"}})
            } else {
                written.await.unwrap();
                json!({"id":first["id"],"error":{"code":-32603,"message":"opaque provider failure"}})
            };
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
            line.clear();
            peer.read_line(&mut line).await.unwrap();
            let next: Value = serde_json::from_str(&line).unwrap();
            peer.get_mut()
                .write_all(
                    format!(
                        "{}\n",
                        json!({"id":next["id"],"result":{"stopReason":"end_turn"}})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            peer
        });
        assert!(matches!(
            super::super::super::transport::send(
                &fixture.service,
                &tab,
                &AgentInput::Framed(frame),
                Some(&first.send_id),
            ),
            Err(DeliveryError::SubmitUnconfirmed { .. })
        ));
        let early = session.cache.lock().snapshot();
        assert_eq!(early.iter().filter(|row| row.kind == "user").count(), 1);
        assert_eq!(early[0].content, "opaque cancelled prompt");
        assert_eq!(early[0].inbound_kind.as_deref(), Some("cli"));
        let original_id = early[0].id.clone();
        if cancelled {
            assert!(super::super::super::controls::interrupt(&fixture.service, &tab).unwrap());
        } else {
            release.send(()).unwrap();
        }
        crate::async_runtime::block_on(wait_retired(&session, &first.send_id));
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&first.send_id)
                .unwrap()
                .state,
            crate::service::chat_send_transaction::ChatSendState::Unknown
        );
        let rows = session.cache.lock().snapshot();
        let users: Vec<_> = rows.iter().filter(|row| row.kind == "user").collect();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id, original_id);
        assert_eq!(users[0].content, "opaque cancelled prompt");
        assert_eq!(users[0].inbound_kind.as_deref(), Some("cli"));
        assert_eq!(users[0].content, expected.content);
        assert_eq!(users[0].segments, expected.segments);
        if cancelled {
            assert!(rows.iter().all(|row| row.kind != "status"));
            crate::async_runtime::block_on(confirm_echo(
                &fixture.service,
                &session,
                &first.send_id,
                &wire,
            ));
            assert_eq!(
                fixture
                    .service
                    .chat_send_ledger
                    .status(&first.send_id)
                    .unwrap()
                    .state,
                crate::service::chat_send_transaction::ChatSendState::Sent
            );
            assert_eq!(
                session
                    .cache
                    .lock()
                    .snapshot()
                    .iter()
                    .filter(|row| row.kind == "user")
                    .count(),
                1
            );
        }
        let next = fixture
            .service
            .chat_send_ledger
            .try_start(&tab, "opaque next prompt", None)
            .unwrap();
        let result = super::super::super::transport::send(
            &fixture.service,
            &tab,
            &AgentInput::Human(HumanInput::composer("opaque next prompt")),
            Some(&next.send_id),
        );
        assert!(matches!(
            result,
            Ok(()) | Err(DeliveryError::SubmitUnconfirmed { .. })
        ));
        crate::async_runtime::block_on(wait_retired(&session, &next.send_id));
        assert!(matches!(
            fixture
                .service
                .chat_send_ledger
                .status(&next.send_id)
                .unwrap()
                .state,
            crate::service::chat_send_transaction::ChatSendState::Unknown
                | crate::service::chat_send_transaction::ChatSendState::Written
        ));
        crate::async_runtime::block_on(confirm_echo(
            &fixture.service,
            &session,
            &next.send_id,
            "opaque next prompt",
        ));
        let rows = session.cache.lock().snapshot();
        let users: Vec<_> = rows.iter().filter(|row| row.kind == "user").collect();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].id, original_id);
        assert_eq!(users[0].content, "opaque cancelled prompt");
        assert_eq!(users[0].inbound_kind.as_deref(), Some("cli"));
        assert_eq!(users[0].content, expected.content);
        assert_eq!(users[1].content, "opaque next prompt");
        assert!(users[0].seq < users[1].seq);
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&next.send_id)
                .unwrap()
                .state,
            crate::service::chat_send_transaction::ChatSendState::Sent
        );
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
}

async fn wait_retired(session: &Arc<Session>, turn: &str) {
    let mut changes = session.control_changes.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            changes.borrow_and_update();
            if session.turn_requests.lock().is_empty()
                && session.completions.lock().contains("own-thread", turn)
            {
                break;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

async fn confirm_echo(svc: &ServiceContext, session: &Arc<Session>, send: &str, text: &str) {
    let epoch = session.context.lock().epoch;
    subscriber::publish_events(
        svc,
        session,
        epoch,
        vec![
            AgentRuntimeEvent::ChatItem {
                thread_id: "own-thread".into(),
                item: ChatMsg {
                    id: format!("user-{send}"),
                    kind: "user".into(),
                    content: text.into(),
                    ..Default::default()
                },
            },
            AgentRuntimeEvent::SendEvidence {
                thread_id: "own-thread".into(),
                send_id: send.into(),
                turn_id: send.into(),
                item_id: Some(format!("user-{send}")),
            },
        ],
        10,
    )
    .await
    .unwrap();
}
