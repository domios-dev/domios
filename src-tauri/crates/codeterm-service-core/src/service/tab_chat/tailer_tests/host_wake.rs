//! Delivery must not depend on file notifications: these tailers run with a
//! watch that never fires, so only host wakes can make them read.

use super::large_sessions::{append, registry_with_provider_at, PROVIDER_ID};
use super::*;
use crate::service::tab_chat::file_events::WatchGuard;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

const TAB: &str = "host-wake";

#[derive(Debug, PartialEq, Eq)]
struct Range {
    start: u64,
    end: u64,
    rows: Vec<String>,
}

fn range_of(messages: &[RichMessage], start: u64, end: u64) -> Range {
    Range {
        start,
        end,
        rows: messages.iter().filter_map(|m| m.uuid.clone()).collect(),
    }
}

struct Recorder {
    ranges: Mutex<Sender<Range>>,
}

impl DeltaSink for Recorder {
    fn emit(&self, _: &str, messages: Vec<RichMessage>, start_offset: u64, offset: u64) {
        let _ = self
            .ranges
            .lock()
            .send(range_of(&messages, start_offset, offset));
    }
}

fn unwatched(_: &Path, _: SyncSender<()>) -> Result<Option<WatchGuard>, String> {
    Ok(None)
}

fn row(kind: &str, uuid: &str) -> Vec<u8> {
    format!(
        "{{\"type\":\"{kind}\",\"uuid\":\"{uuid}\",\"message\":{{\"role\":\"{kind}\",\"content\":\"{uuid}\"}}}}\n"
    )
    .into_bytes()
}

fn silent(kind: &str, bytes: usize) -> Vec<u8> {
    format!(
        "{{\"type\":\"{kind}\",\"pad\":\"{}\"}}\n",
        "x".repeat(bytes)
    )
    .into_bytes()
}

fn attach(
    path: &Path,
    watch: super::super::tailer::WatchSource,
    sink: Arc<dyn DeltaSink>,
    chats: &ChatRegistry,
) {
    let (ready, readiness) = mpsc::channel();
    let handle = super::super::tailer::spawn_tailer_watched_by(
        TailConfig {
            tab_id: TAB.into(),
            provider_id: PROVIDER_ID.into(),
            cwd: "/proj/host-wake".into(),
            session_id: "session".into(),
            start_offset: 0,
        },
        registry_with_provider_at(path),
        sink,
        ready,
        watch,
    );
    readiness.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(chats.insert(TAB.into(), "session".into(), None, "chat".into(), handle));
}

fn recorder() -> (Arc<dyn DeltaSink>, Receiver<Range>) {
    let (ranges, received) = mpsc::channel();
    (
        Arc::new(Recorder {
            ranges: Mutex::new(ranges),
        }),
        received,
    )
}

/// Ranges up to `end`, asserted contiguous from `from`.
fn ranges_until(received: &Receiver<Range>, from: u64, end: u64) -> Vec<Range> {
    let mut ranges: Vec<Range> = Vec::new();
    let mut committed = from;
    while committed < end {
        let range = received.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(range.start, committed, "live ranges stay contiguous");
        committed = range.end;
        ranges.push(range);
    }
    assert_eq!(committed, end);
    ranges
}

fn rows(ranges: &[Range]) -> Vec<String> {
    ranges.iter().flat_map(|range| range.rows.clone()).collect()
}

#[test]
fn turn_end_wake_emits_the_reply_without_file_events() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("session.jsonl");
    let chats = ChatRegistry::default();
    let (sink, received) = recorder();
    attach(&path, unwatched, sink, &chats);

    std::fs::write(&path, row("user", "u1")).unwrap();
    let prompt_end = std::fs::metadata(&path).unwrap().len();
    chats.wake(TAB);
    assert_eq!(rows(&ranges_until(&received, 0, prompt_end)), ["u1"]);

    let reply_end = append(&path, &row("assistant", "a1"));
    chats.wake(TAB);
    assert_eq!(
        ranges_until(&received, prompt_end, reply_end),
        [Range {
            start: prompt_end,
            end: reply_end,
            rows: vec!["a1".into()],
        }]
    );
    chats.shutdown_tab(TAB);
}

static ARMED_BEFORE_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn counted_unwatched(path: &Path, wake: SyncSender<()>) -> Result<Option<WatchGuard>, String> {
    ARMED_BEFORE_DIRECTORY.fetch_add(1, Ordering::SeqCst);
    unwatched(path, wake)
}

#[test]
fn attach_before_the_session_file_exists_emits_the_reply_after_large_silent_rows() {
    let tmp = TempDir::new().unwrap();
    let directory = tmp.path().join("projects").join("session-dir");
    std::fs::create_dir(tmp.path().join("projects")).unwrap();
    let path = directory.join("session.jsonl");
    let chats = ChatRegistry::default();
    let (sink, received) = recorder();
    attach(&path, counted_unwatched, sink, &chats);
    assert_eq!(ARMED_BEFORE_DIRECTORY.load(Ordering::SeqCst), 1);

    std::fs::create_dir(&directory).unwrap();
    std::fs::write(&path, row("user", "u1")).unwrap();
    append(&path, &silent("attachment", 145_000));
    append(&path, &silent("attachment", 35_000));
    append(&path, &silent("api-request-shape", 91_000));
    append(&path, &silent("api-request-blob", 74_000));
    let reply_end = append(&path, &row("assistant", "a1"));
    chats.wake(TAB);
    assert_eq!(rows(&ranges_until(&received, 0, reply_end)), ["u1", "a1"]);
    // The watch moves onto the materialized directory once; later wakes only read.
    assert_eq!(ARMED_BEFORE_DIRECTORY.load(Ordering::SeqCst), 2);

    let next_end = append(&path, &row("assistant", "a2"));
    chats.wake(TAB);
    chats.wake(TAB);
    assert_eq!(rows(&ranges_until(&received, reply_end, next_end)), ["a2"]);
    assert_eq!(ARMED_BEFORE_DIRECTORY.load(Ordering::SeqCst), 2);
    chats.shutdown_tab(TAB);
}

/// Appends the next row and wakes twice from inside the emit, i.e. while the
/// tailer's read is still in flight.
struct InFlightWaker {
    ranges: Mutex<Sender<Range>>,
    chats: Arc<ChatRegistry>,
    path: std::path::PathBuf,
    pending: Mutex<Vec<Vec<u8>>>,
}

impl DeltaSink for InFlightWaker {
    fn emit(&self, _: &str, messages: Vec<RichMessage>, start_offset: u64, offset: u64) {
        let _ = self
            .ranges
            .lock()
            .send(range_of(&messages, start_offset, offset));
        if let Some(next) = self.pending.lock().pop() {
            append(&self.path, &next);
            self.chats.wake(TAB);
            self.chats.wake(TAB);
        }
    }
}

#[test]
fn wakes_during_an_in_flight_read_emit_every_row_once_in_contiguous_ranges() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("session.jsonl");
    let replies: Vec<String> = (1..=6).map(|n| format!("a{n}")).collect();
    let mut pending: Vec<Vec<u8>> = replies
        .iter()
        .flat_map(|uuid| [silent("system", 900), row("assistant", uuid)])
        .collect();
    pending.reverse();
    let prompt = row("user", "u1");
    let total = (prompt.len() + pending.iter().map(Vec::len).sum::<usize>()) as u64;
    let chats = ChatRegistry::new();
    let (ranges, received) = mpsc::channel();
    let sink = Arc::new(InFlightWaker {
        ranges: Mutex::new(ranges),
        chats: chats.clone(),
        path: path.clone(),
        pending: Mutex::new(pending),
    });
    attach(&path, unwatched, sink, &chats);

    std::fs::write(&path, prompt).unwrap();
    chats.wake(TAB);
    let mut expected = vec!["u1".to_string()];
    expected.extend(replies);
    assert_eq!(rows(&ranges_until(&received, 0, total)), expected);
    chats.shutdown_tab(TAB);
}
