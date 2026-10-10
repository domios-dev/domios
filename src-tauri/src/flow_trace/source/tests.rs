use super::*;
use crate::service::chat_send_transaction::{ChatSendLedger, SendLiveBinding};
use crate::service::codex_rpc::{normalize::Normalizer, protocol::Frame, send};
use codeterm_types::trace::{gaps, read, select, Sink};
use serde_json::json;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

#[test]
fn captured_trace_link_preserves_source_and_bounded_display_detail() {
    let source = "é".repeat(300);
    let events = crate::flow_trace::test_capture::capture(|| {
        crate::flow_trace::record(
            "send",
            "trace.link",
            "tab",
            "provider",
            "exact_append_window",
            Some(&source),
        );
    });
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].related_id.as_deref(), Some(source.as_str()));
    assert_eq!(events[0].detail, "é".repeat(240));
}

fn persisted(
    events: Vec<codeterm_types::trace::Event>,
    id: &str,
) -> Vec<codeterm_types::trace::Event> {
    let dir = tempfile::tempdir().unwrap();
    let mut sink = Sink::open(dir.path(), true).unwrap();
    for event in events {
        sink.append(&event).unwrap();
    }
    drop(sink);
    select(&read(dir.path()).unwrap(), id, 0)
}

#[test]
fn rpc_send_ingress_protocol_answer_reaches_sink_with_exact_client_turn_link() {
    let ledger = ChatSendLedger::default();
    let mut id = String::new();
    let events = crate::flow_trace::test_capture::capture(|| {
        id = ledger.try_start("tab", "opaque", None).unwrap().send_id;
        assert!(ledger.bind_rpc_send(
            &id,
            SendLiveBinding {
                session_id: Some("session".into()),
                binding_nonce: Some("nonce".into()),
            }
        ));
        let future = send::submit(
            send::Origin::Agent,
            None,
            send::start_params("session", &id, "opaque", None, json!({})),
            |method, request| {
                assert_eq!(method, "turn/start");
                assert_eq!(request["clientUserMessageId"], id);
                std::future::ready(Ok(json!({"turn":{"id":"turn"}})))
            },
        );
        let mut future = std::pin::pin!(future);
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(_))
        ));
        ledger.mark_rpc_accepted(&id).unwrap();
        let later = ledger.try_start("tab", "later", None).unwrap();
        assert_ne!(later.send_id, id);
        let user = json!({"threadId":"session","turnId":"turn","item":{"id":"user","type":"userMessage",
            "clientId":id,"content":[{"type":"text","text":"opaque"}]}});
        let (session, client) = send::user_client_id("item/completed", &user).unwrap();
        ledger.trace_rpc_turn("tab", session, "wrong", client, "turn", "provider");
        ledger.trace_rpc_turn(
            "tab",
            session,
            "nonce",
            client,
            user["turnId"].as_str().unwrap(),
            "provider",
        );
        ledger
            .confirm_rpc_send("tab", session, "nonce", client)
            .unwrap();
        let mut normalizer = Normalizer::default();
        let frame = Frame::Notification {
            method: "item/completed".into(),
            params: json!({"threadId":"session","turnId":"turn","item":{"id":"answer","type":"agentMessage","text":"opaque answer"}}),
        };
        for message in normalizer.apply_all(&frame) {
            SourceCursor::rpc("tab", "provider", "session", Some("nonce"), &message);
        }
        SourceCursor::rpc(
            "tab",
            "provider",
            "foreign",
            Some("nonce"),
            &ChatMsg {
                kind: "assistant".into(),
                tool_flow_id: Some("turn".into()),
                ..Default::default()
            },
        );
    });
    let chain = persisted(events, &id);
    assert!(chain.iter().all(|event| event.trace_id == id));
    for stage in [
        "send.started",
        "send.written",
        "send.delivered",
        "transcript.assistant",
    ] {
        assert!(
            chain.iter().any(|event| event.stage == stage),
            "missing {stage}"
        );
    }
    assert_eq!(
        chain
            .iter()
            .filter(|event| event.stage == "trace.link")
            .count(),
        1
    );
    assert_eq!(
        chain
            .iter()
            .filter(|event| event.stage == "transcript.assistant")
            .count(),
        1
    );
    assert!(gaps(&chain).is_empty());
}

#[test]
fn late_exact_source_link_resolves_an_answer_recorded_before_confirmation() {
    let mut id = String::new();
    let events = crate::flow_trace::test_capture::capture(|| {
        let flow = PtyFlow::start(None, "tab", "provider").unwrap();
        id = flow.id.clone();
        let anchor = SendEvidenceAnchor {
            provider_id: "provider".into(),
            cwd: "/".into(),
            session_id: "session".into(),
            window: crate::service::chat_send_transaction::EvidenceWindow::Jsonl {
                transcript_offset: 10,
            },
            turn_normalization: Default::default(),
        };
        flow.written();
        let mut source = SourceCursor::default();
        source.pty(
            "tab",
            "provider",
            "session",
            Some("nonce"),
            &[
                ChatMsg {
                    id: "user".into(),
                    kind: "user".into(),
                    ..Default::default()
                },
                ChatMsg {
                    id: "answer".into(),
                    kind: "assistant".into(),
                    ..Default::default()
                },
            ],
        );
        flow.confirmed(&anchor, Some("nonce"), Some("user"));
    });
    let chain = persisted(events, &id);
    assert!(chain.iter().all(|event| event.trace_id == id));
    assert!(gaps(&chain).is_empty());
}
