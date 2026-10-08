use super::*;
use codeterm_types::trace::{gaps, read, select, Sink};
use std::io::Write;

#[test]
fn plain_pty_ingress_exact_append_window_answer_reaches_the_dev_sink() {
    let fixture = Fixture::new();
    fixture.append_user_turn("session", "opaque");
    let anchor = fixture.anchor_now("session");
    let mut exact = ExactTurnCursor::for_anchor(Some(&anchor));
    let mut flow_id = String::new();
    let events = crate::flow_trace::test_capture::capture(|| {
        let flow = crate::flow_trace::PtyFlow::start(None, "tab", "windowed").unwrap();
        assert!(!contains_exact_user_turn_since(
            &fixture.registry,
            &anchor,
            "opaque",
            &mut exact
        ));
        flow.written();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(fixture.session_path("session"))
            .unwrap();
        for row in [
            serde_json::json!({"id":"accepted-user","role":"user","content":"opaque"}),
            serde_json::json!({"id":"answer","role":"assistant","content":"opaque answer"}),
        ] {
            writeln!(file, "{row}").unwrap();
        }
        assert!(contains_exact_user_turn_since(
            &fixture.registry,
            &anchor,
            "opaque",
            &mut exact
        ));
        assert_eq!(exact.matched_user_id.as_deref(), Some("accepted-user-0"));
        flow.confirmed(&anchor, Some("nonce"), exact.matched_user_id.as_deref());
        let EvidenceWindow::Jsonl { transcript_offset } = anchor.window else {
            unreachable!()
        };
        let delta = fixture
            .registry
            .with_provider("windowed", |provider| {
                provider.parse_session_delta(&fixture.session_path("session"), transcript_offset)
            })
            .unwrap();
        let messages = delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(index, message)| {
                crate::service::tab_chat::rich_to_chat_msgs_live(message, index)
            })
            .collect::<Vec<_>>();
        let mut source = crate::flow_trace::SourceCursor::default();
        source.pty("tab", "windowed", "session", Some("stale"), &messages);
        source.pty("tab", "windowed", "session", Some("nonce"), &messages);
    });
    let root = events
        .iter()
        .find(|event| event.stage == "send.started")
        .unwrap();
    flow_id.push_str(&root.trace_id);
    assert_eq!(root.outcome, "fresh_unledgered_pty");
    let dir = tempfile::tempdir().unwrap();
    let mut sink = Sink::open(dir.path(), true).unwrap();
    for event in events {
        sink.append(&event).unwrap();
    }
    drop(sink);
    let chain = select(&read(dir.path()).unwrap(), &flow_id, 0);
    assert!(chain.iter().all(|event| event.trace_id == flow_id));
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
            .filter(|event| event.stage == "transcript.assistant")
            .count(),
        1
    );
    assert!(gaps(&chain).is_empty());
    let missing = chain
        .into_iter()
        .filter(|event| event.stage != "transcript.assistant")
        .collect::<Vec<_>>();
    assert!(gaps(&missing)
        .iter()
        .any(|gap| gap.contains("no assistant transcript append")));
}
