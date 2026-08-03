use super::*;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use codeterm_plugin::jsonl_window::{read_records_from, DELTA_CHUNK_BYTES};

use crate::agent::chat_types::{MessageBlock, SessionDelta};
use crate::agent::provider::AgentProvider;

/// Parses `{"text":"..."}` records through the same bounded reader the real JS
/// provider uses, and counts how many bytes each snapshot actually pulled.
struct WindowProvider {
    reads: AtomicUsize,
    bytes: AtomicUsize,
}

impl WindowProvider {
    fn new() -> Self {
        Self {
            reads: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        }
    }
}

impl AgentProvider for WindowProvider {
    fn id(&self) -> &str {
        "window-test"
    }
    fn display_name(&self) -> &str {
        "Window Test"
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn parse_session_delta(&self, path: &std::path::Path, from_offset: u64) -> SessionDelta {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let Ok(chunk) = read_records_from(path, from_offset, DELTA_CHUNK_BYTES) else {
            return SessionDelta {
                new_offset: from_offset,
                messages: vec![],
            };
        };
        self.bytes.fetch_add(chunk.bytes.len(), Ordering::Relaxed);
        let messages = String::from_utf8_lossy(&chunk.bytes)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|v| RichMessage {
                role: "assistant".into(),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text": v.get("text").and_then(|t| t.as_str()).unwrap_or("")}),
                }],
                ..Default::default()
            })
            .collect();
        SessionDelta {
            new_offset: chunk.new_offset,
            messages,
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    registry: Arc<AgentRegistry>,
    total: u64,
    per_record: u64,
}

/// Writes `count` fixed-width records so offsets are exactly predictable.
fn fixture(name: &str, count: usize, pad: usize) -> Fixture {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join(format!("{name}.jsonl"));
    let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut per_record = 0u64;
    for i in 0..count {
        let line = format!("{{\"text\":\"{i:08}{}\"}}\n", "p".repeat(pad));
        if per_record == 0 {
            per_record = line.len() as u64;
        }
        assert_eq!(per_record, line.len() as u64, "records must be fixed width");
        f.write_all(line.as_bytes()).unwrap();
    }
    f.flush().unwrap();
    drop(f);

    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(WindowProvider::new()));
    Fixture {
        total: std::fs::metadata(&path).unwrap().len(),
        _dir: dir,
        path,
        registry: Arc::new(registry),
        per_record,
    }
}

fn text_of(msg: &ChatMsg) -> String {
    msg.content.clone()
}

#[test]
fn small_session_snapshots_the_whole_transcript() {
    let fx = fixture("small", 200, 32);
    assert!(fx.total < SNAPSHOT_CAP_BYTES);

    let snap = snapshot(&fx.registry, "window-test", &fx.path);
    assert_eq!(snap.start_offset, 0);
    assert!(!snap.truncated());
    assert_eq!(snap.total, fx.total);
    assert_eq!(snap.bytes_read, fx.total);
    assert_eq!(snap.messages.len(), 200);
    assert!(text_of(&snap.messages[0]).starts_with("00000000"));
    assert!(text_of(snap.messages.last().unwrap()).starts_with("00000199"));
}

/// The production regression: a session file far past the cap must yield a
/// bounded tail, not a whole-file read.
#[test]
fn oversized_session_reads_only_a_bounded_tail() {
    let pad = 4000;
    let per = pad as u64 + 8 + 12;
    let count = ((SNAPSHOT_CAP_BYTES * 3) / per) as usize;
    let fx = fixture("oversized", count, pad);
    assert!(fx.total > SNAPSHOT_CAP_BYTES * 2, "fixture must exceed cap");

    let snap = snapshot(&fx.registry, "window-test", &fx.path);
    assert!(snap.truncated());
    assert!(
        snap.bytes_read <= SNAPSHOT_WINDOW_BYTES + SNAPSHOT_ANCHOR_STRIDE_BYTES,
        "read {} bytes, window is {} plus at most one anchor stride",
        snap.bytes_read,
        SNAPSHOT_WINDOW_BYTES
    );
    assert!(
        snap.bytes_read * 4 < fx.total,
        "bounded read must be a small fraction of {} bytes",
        fx.total
    );
    // Boundary preserved: the window starts on a record and ends at EOF.
    assert_eq!(snap.start_offset % fx.per_record, 0);
    assert_eq!(snap.start_offset + snap.bytes_read, fx.total);

    // Last-N semantics: the window holds the TAIL of the transcript.
    let expected = (fx.total - snap.start_offset) / fx.per_record;
    assert_eq!(snap.messages.len() as u64, expected);
    let last = text_of(snap.messages.last().unwrap());
    assert!(last.starts_with(&format!("{:08}", count - 1)), "got {last}");
    let first = text_of(&snap.messages[0]);
    assert!(
        first > format!("{:08}", count / 2),
        "window must be the tail, got {first}"
    );
}

/// A window wider than one delta chunk must drain across passes without
/// dropping, duplicating, or splitting a record.
/// A bounded window must re-anchor only when the file grows past a stride:
/// row ids are ordinals inside their window, so an anchor that moved on every
/// attach would re-key the whole overlap on each Terminal -> Chat switch.
#[test]
fn snapshot_anchor_is_stable_while_the_session_grows_within_a_stride() {
    let pad = 4000;
    let per = pad as u64 + 8 + 12;
    let count = ((SNAPSHOT_CAP_BYTES * 3) / per) as usize;
    let fx = fixture("anchor", count, pad);

    let first = snapshot_start_offset(&fx.path, fx.total);
    assert!(first > 0, "oversized session must use a bounded window");
    let grown = fx.total + SNAPSHOT_ANCHOR_STRIDE_BYTES / 4;
    assert_eq!(
        snapshot_start_offset(&fx.path, grown),
        first,
        "growth below one stride must reuse the same window anchor"
    );
    let advanced = fx.total + SNAPSHOT_ANCHOR_STRIDE_BYTES;
    assert!(
        snapshot_start_offset(&fx.path, advanced) > first,
        "growth past a stride re-anchors the window forward"
    );
}

#[test]
fn window_wider_than_the_delta_chunk_drains_completely() {
    let pad = 512;
    let per = pad as u64 + 8 + 12;
    let count = ((DELTA_CHUNK_BYTES + DELTA_CHUNK_BYTES / 2) / per) as usize;
    let fx = fixture("drain", count, pad);
    assert!(fx.total > DELTA_CHUNK_BYTES);

    let (rich, end) = parse_window(&fx.registry, "window-test", &fx.path, 0, fx.total);
    assert_eq!(end, fx.total);
    assert_eq!(rich.len(), count);
}

#[test]
fn trailing_partial_record_is_held_back() {
    let fx = fixture("partial", 10, 16);
    let complete = fx.total;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&fx.path)
        .unwrap();
    f.write_all(b"{\"text\":\"unterminated").unwrap();
    f.flush().unwrap();
    drop(f);

    let snap = snapshot(&fx.registry, "window-test", &fx.path);
    assert_eq!(snap.messages.len(), 10);
    assert_eq!(snap.start_offset + snap.bytes_read, complete);
    // The tailer resumes at file length so the partial line is re-read whole.
    assert!(snap.total > complete);
}

#[test]
fn missing_file_snapshots_empty_without_panicking() {
    let fx = fixture("missing", 4, 16);
    std::fs::remove_file(&fx.path).unwrap();
    let snap = snapshot(&fx.registry, "window-test", &fx.path);
    assert_eq!(snap.total, 0);
    assert_eq!(snap.start_offset, 0);
    assert!(snap.messages.is_empty());
}

#[test]
fn parse_window_terminates_when_a_provider_stops_advancing() {
    let fx = fixture("stuck", 4, 16);
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(StuckProvider));
    let registry = Arc::new(registry);
    let (rich, end) = parse_window(&registry, "stuck", &fx.path, 0, fx.total);
    assert!(rich.is_empty());
    assert_eq!(end, 0);
}

struct StuckProvider;

impl AgentProvider for StuckProvider {
    fn id(&self) -> &str {
        "stuck"
    }
    fn display_name(&self) -> &str {
        "Stuck"
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn parse_session_delta(&self, _: &std::path::Path, from_offset: u64) -> SessionDelta {
        SessionDelta {
            new_offset: from_offset,
            messages: vec![],
        }
    }
}
