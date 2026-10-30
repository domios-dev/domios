use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::tab_chat::{session_window, structured_sink};
use std::io::Write;
use std::sync::mpsc;

fn append(path: &std::path::Path, bytes: &[u8]) -> u64 {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file.metadata().unwrap().len()
}

#[test]
fn bounded_large_session_streams_silent_ranges_and_rows_after_reload() {
    session_streams_silent_ranges_and_rows_after_reload(true);
}

#[test]
fn small_session_streams_silent_ranges_and_rows_after_reload() {
    session_streams_silent_ranges_and_rows_after_reload(false);
}

fn session_streams_silent_ranges_and_rows_after_reload(large: bool) {
    let root = TempDir::new().unwrap();
    let directory = root.path().join("-proj-large-session");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("large-session.jsonl");
    let blob = if large {
        format!(
            "{{\"type\":\"api-request-blob\",\"data\":\"{}\"}}\n",
            "x".repeat(1_300_000)
        )
    } else {
        [
            "attachment",
            "api-request-blob",
            "api-request",
            "mode",
            "permission-mode",
            "atis-latch",
            "last-prompt",
            "ai-title",
            "queue-operation",
            "file-history-snapshot",
            "tools-used",
        ]
        .iter()
        .map(|kind| format!("{{\"type\":\"{kind}\"}}\n"))
        .collect::<String>()
    };
    let history = b"{\"type\":\"user\",\"uuid\":\"history\",\"message\":{\"role\":\"user\",\"content\":\"snapshot row\"}}\n";
    let mut file = std::fs::File::create(&path).unwrap();
    let history_blobs = if large { 16 } else { 1 };
    for _ in 0..history_blobs {
        file.write_all(blob.as_bytes()).unwrap();
    }
    file.write_all(history).unwrap();
    file.flush().unwrap();
    drop(file);

    let registry = registry_with_claude_at(root.path());
    let snapshot = session_window::try_snapshot(&registry, "claude", &path).unwrap();
    if large {
        assert!(snapshot.total > 20_000_000);
        assert!(snapshot.start_offset > 0);
    } else {
        assert!(snapshot.total < 1024);
        assert_eq!(snapshot.start_offset, 0);
    }
    assert_eq!(snapshot.end_offset, snapshot.total);
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].content, "snapshot row");

    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (sent, received) = mpsc::channel();
    let listener = fixture
        .ui
        .listen(&format!("tab-chat-msg-{tab}"), move |payload| {
            sent.send(payload.clone()).unwrap();
        });
    let sink = structured_sink::make_flatten_sink(
        fixture.service.lifecycle_journal.clone(),
        &fixture.service,
        fixture.service.mux.clone(),
        snapshot.messages.len(),
        Some("large-session".into()),
        None,
        None,
    );
    let (ready, readiness) = mpsc::channel();
    let handle = super::super::tailer::spawn_tailer_with_ready(
        TailConfig {
            tab_id: tab.clone(),
            provider_id: "claude".into(),
            cwd: "/proj/large-session".into(),
            session_id: "large-session".into(),
            start_offset: snapshot.end_offset,
        },
        registry.clone(),
        sink,
        ready,
    );
    readiness.recv_timeout(Duration::from_secs(30)).unwrap();
    let chats = ChatRegistry::default();
    assert!(chats.insert(
        tab.clone(),
        "large-session".into(),
        None,
        "first".into(),
        handle
    ));

    let mut silent = Vec::new();
    for _ in 0..9 {
        silent.extend_from_slice(blob.as_bytes());
    }
    let silent_end = append(&path, &silent);
    let mut committed = snapshot.end_offset;
    while committed < silent_end {
        let event = received.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(event["start_offset"].as_u64(), Some(committed));
        assert_eq!(event["messages"], serde_json::json!([]));
        let next = event["offset"].as_u64().unwrap();
        assert!(next > committed && next <= silent_end);
        committed = next;
    }
    let reply = b"{\"type\":\"assistant\",\"uuid\":\"reply\",\"message\":{\"role\":\"assistant\",\"content\":\"live reply\"}}\n";
    let reply_end = append(&path, reply);
    let event = received.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(event["start_offset"], silent_end);
    assert_eq!(event["offset"], reply_end);
    assert_eq!(event["messages"][0]["content"], "live reply");

    chats.stop(&tab, "first");
    let detached = b"{\"type\":\"user\",\"uuid\":\"detached\",\"message\":{\"role\":\"user\",\"content\":\"while detached\"}}\n";
    let detached_end = append(&path, detached);
    let event = received.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(event["messages"][0]["content"], "while detached");
    let reload = session_window::try_snapshot(&registry, "claude", &path).unwrap();
    assert_eq!(reload.end_offset, detached_end);
    assert!(reload
        .messages
        .iter()
        .any(|row| row.content == "live reply"));
    assert!(reload
        .messages
        .iter()
        .any(|row| row.content == "while detached"));
    assert!(chats.acquire(&tab, "large-session", None, "reload"));
    let next = b"{\"type\":\"assistant\",\"uuid\":\"after-reload\",\"message\":{\"role\":\"assistant\",\"content\":\"after reload\"}}\n";
    let next_end = append(&path, next);
    let event = received.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(event["start_offset"], detached_end);
    assert_eq!(event["offset"], next_end);
    assert_eq!(event["messages"][0]["content"], "after reload");
    chats.shutdown_tab(&tab);
    fixture.ui.unlisten(listener);
}

#[test]
fn acquire_wakes_retained_event_tailer_without_a_file_event() {
    let registry = ChatRegistry::default();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let (wake, events) = mpsc::sync_channel(1);
    let (observed, observation) = mpsc::channel();
    let handle = super::super::tailer::TailerHandle::event_driven(
        stop,
        Arc::new(std::sync::atomic::AtomicU64::new(42)),
        std::thread::spawn(move || {
            while events.recv().is_ok() {
                if worker_stop.load(Ordering::Acquire) {
                    return;
                }
                observed.send(()).unwrap();
            }
        }),
        wake,
    );
    assert!(registry.insert("tab".into(), "session".into(), None, "old".into(), handle));
    registry.stop("tab", "old");
    assert!(registry.acquire("tab", "session", None, "reload"));
    observation.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(registry.current_offset("tab"), Some(42));
    registry.shutdown_tab("tab");
}
