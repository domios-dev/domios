use super::*;
use codeterm_types::trace::{gaps, read, select, Sink};
use std::io::Write;

#[test]
fn fresh_pty_source_registration_before_materialization_reaches_the_sink() {
    let fixture = Fixture::new();
    let path = fixture.session_path("fresh");
    let source = crate::service::tab_chat::registry::JsonlSource::default();
    let anchor = SendEvidenceAnchor {
        window: crate::flow_trace::capture_window(&source, "fresh", &path).unwrap(),
        ..fixture.anchor_now("fresh")
    };
    let events = crate::flow_trace::test_capture::capture(|| {
        let flow = crate::flow_trace::PtyFlow::start(None, "tab", "windowed").unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let watcher = crate::flow_trace::register_source(&path, tx).unwrap();
        assert!(!path.exists());
        fixture
            .registry
            .register_provider(Box::new(WindowedJsonlProvider {
                root: fixture.root.join("reconstructed"),
                parse_offsets: fixture.parse_offsets.clone(),
            }));
        assert_ne!(
            fixture
                .registry
                .with_provider("windowed", |provider| {
                    provider.session_jsonl_path("/proj", "fresh")
                })
                .flatten()
                .unwrap(),
            path
        );
        let mut observation =
            crate::flow_trace::UnledgeredObservation::new(flow.clone(), "opaque".into(), &anchor);
        let mut cursor = crate::flow_trace::SourceCursor::default();
        assert!(!observation.observe(&anchor, Some("nonce"), &[], &cursor));
        flow.written();
        let mut file = std::fs::File::create(&path).unwrap();
        for row in [
            serde_json::json!({"id":"fresh-user","role":"user","content":"opaque"}),
            serde_json::json!({"id":"fresh-answer","role":"assistant","content":"answer"}),
        ] {
            writeln!(file, "{row}").unwrap();
        }
        file.flush().unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        let delta = fixture
            .registry
            .with_provider("windowed", |provider| {
                provider.parse_session_delta(&path, 0)
            })
            .unwrap();
        let messages = delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(index, rich)| crate::service::tab_chat::rich_to_chat_msgs_live(rich, index))
            .collect::<Vec<_>>();
        cursor.pty("tab", "windowed", "fresh", Some("nonce"), &messages);
        assert!(observation.observe(&anchor, Some("nonce"), &messages, &cursor));
        drop(watcher);
    });
    let id = events
        .iter()
        .find(|event| event.stage == "send.started")
        .unwrap()
        .trace_id
        .clone();
    let dir = tempfile::tempdir().unwrap();
    let mut sink = Sink::open(dir.path(), true).unwrap();
    for event in events {
        sink.append(&event).unwrap();
    }
    drop(sink);
    let chain = select(&read(dir.path()).unwrap(), &id, 0);
    assert!(gaps(&chain).is_empty());
    assert_eq!(
        chain
            .iter()
            .filter(|event| event.stage == "transcript.assistant")
            .count(),
        1
    );
    let link = chain
        .iter()
        .find(|event| event.stage == "trace.link")
        .expect("fresh source must link to the send");
    assert_eq!(link.trace_id, id);
    let expected = serde_json::json!(["pty", "tab", "fresh", "nonce", "fresh-user-0"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(link.related_id.as_deref().unwrap()).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&link.detail).unwrap(),
        expected
    );
}

#[test]
fn plain_pty_ingress_exact_append_window_answer_reaches_the_dev_sink() {
    let fixture = Fixture::new();
    fixture.append_user_turn("session", "opaque");
    let anchor = fixture.anchor_now("session");
    let mut flow_id = String::new();
    let events = crate::flow_trace::test_capture::capture(|| {
        let flow = crate::flow_trace::PtyFlow::start(None, "tab", "windowed").unwrap();
        let mut observation =
            crate::flow_trace::UnledgeredObservation::new(flow.clone(), "opaque".into(), &anchor);
        let mut source = crate::flow_trace::SourceCursor::default();
        assert!(!observation.observe(&anchor, Some("nonce"), &[], &source));
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
        source.pty("tab", "windowed", "session", Some("stale"), &messages);
        assert!(!observation.observe(&anchor, Some("nonce"), &messages, &source));
        source.pty("tab", "windowed", "session", Some("nonce"), &messages);
        assert!(observation.observe(&anchor, Some("nonce"), &messages, &source));
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
