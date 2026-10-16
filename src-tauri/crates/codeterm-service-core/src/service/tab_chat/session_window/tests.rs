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
    reads: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
}

impl WindowProvider {
    fn new() -> Self {
        Self {
            reads: Arc::new(AtomicUsize::new(0)),
            bytes: Arc::new(AtomicUsize::new(0)),
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
        self.parse_session_delta_bounded(path, from_offset, DELTA_CHUNK_BYTES)
            .unwrap()
    }

    fn parse_session_delta_bounded(
        &self,
        path: &std::path::Path,
        from_offset: u64,
        budget: u64,
    ) -> Option<SessionDelta> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let Ok(chunk) = read_records_from(path, from_offset, budget.min(DELTA_CHUNK_BYTES)) else {
            return Some(SessionDelta {
                new_offset: from_offset,
                messages: vec![],
            });
        };
        self.bytes.fetch_add(chunk.bytes.len(), Ordering::Relaxed);
        let mut messages: Vec<RichMessage> = String::from_utf8_lossy(&chunk.bytes)
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
        let indexes: Vec<Option<usize>> = (0..messages.len()).map(Some).collect();
        codeterm_plugin::jsonl_window::anchor_rows(
            &mut messages,
            &indexes,
            &chunk.bytes,
            from_offset,
        );
        Some(SessionDelta {
            new_offset: chunk.new_offset,
            messages,
        })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    registry: Arc<AgentRegistry>,
    total: u64,
    per_record: u64,
    reads: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
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
    let provider = WindowProvider::new();
    let reads = provider.reads.clone();
    let bytes = provider.bytes.clone();
    registry.register_provider(Box::new(provider));
    Fixture {
        total: std::fs::metadata(&path).unwrap().len(),
        _dir: dir,
        path,
        registry: Arc::new(registry),
        per_record,
        reads,
        bytes,
    }
}

fn text_of(msg: &ChatMsg) -> String {
    msg.content.clone()
}

#[test]
fn small_session_snapshots_the_whole_transcript() {
    let fx = fixture("small", 100, 32);
    assert!(fx.total < SNAPSHOT_CAP_BYTES);

    let snap = snapshot(&fx.registry, "window-test", &fx.path);
    assert_eq!(snap.start_offset, 0);
    assert!(!snap.truncated());
    assert_eq!(snap.total, fx.total);
    assert_eq!(snap.bytes_read, fx.total);
    assert_eq!(snap.messages.len(), 100);
    assert!(text_of(&snap.messages[0]).starts_with("00000000"));
    assert!(text_of(snap.messages.last().unwrap()).starts_with("00000099"));
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
    assert!(fx.bytes.load(Ordering::Relaxed) as u64 <= SNAPSHOT_WINDOW_BYTES);
    assert!(
        snap.bytes_read <= SNAPSHOT_WINDOW_BYTES,
        "read {} bytes, window is {}",
        snap.bytes_read,
        SNAPSHOT_WINDOW_BYTES
    );
    assert!(
        snap.bytes_read * 2 < fx.total,
        "bounded read must be a small fraction of {} bytes",
        fx.total
    );
    // Boundary preserved: the window starts on a record and ends at EOF.
    assert_eq!(snap.start_offset % fx.per_record, 0);
    assert_eq!(snap.end_offset, fx.total);

    // Last-N semantics: the window holds the TAIL of the transcript.
    let expected = (fx.total - snap.start_offset) / fx.per_record;
    assert_eq!(snap.messages.len() as u64, expected);
    assert!(snap.messages.len() <= super::super::snapshot_rows::SNAPSHOT_ROW_LIMIT);
    let last = text_of(snap.messages.last().unwrap());
    assert!(last.starts_with(&format!("{:08}", count - 1)), "got {last}");
    let first = text_of(&snap.messages[0]);
    assert!(
        first > format!("{:08}", count / 2),
        "window must be the tail, got {first}"
    );
}

#[test]
fn snapshot_window_tracks_the_tail_within_the_byte_budget() {
    let pad = 4000;
    let per = pad as u64 + 8 + 12;
    let count = ((SNAPSHOT_CAP_BYTES * 3) / per) as usize;
    let fx = fixture("anchor", count, pad);

    let first = snapshot_start_offset(&fx.path, fx.total);
    assert!(first > 0, "oversized session must use a bounded window");
    assert!(fx.total - first <= SNAPSHOT_WINDOW_BYTES);
    assert_eq!(
        snapshot_start_offset(&fx.path, fx.total + 1024),
        first,
        "a request past EOF clamps to the actual file"
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
    assert_eq!(fx.reads.load(Ordering::Relaxed), 2);
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

#[test]
fn an_interior_window_passes_its_exact_remaining_budget_to_the_host_reader() {
    let fx = fixture("interior-budget", 100, 64);
    let until = fx.per_record * 3;
    let (rows, end) = parse_window(&fx.registry, "window-test", &fx.path, 0, until);
    assert_eq!(rows.len(), 3);
    assert_eq!(end, until);
    assert_eq!(fx.bytes.load(Ordering::Relaxed) as u64, until);
    assert_eq!(fx.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn backfill_reads_one_megabyte_of_complete_records_without_a_second_raw_copy() {
    let fx = fixture("backfill-budget", 3000, 1000);
    let page = parse_page_before(&fx.registry, "window-test", &fx.path, fx.total).unwrap();
    assert!(page.start_offset > 0);
    assert_eq!(page.end_offset, fx.total);
    assert_eq!(page.start_offset % fx.per_record, 0);
    assert!(fx.bytes.load(Ordering::Relaxed) as u64 <= BACKFILL_PAGE_BYTES);
    assert_eq!(fx.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn missing_backfill_source_is_an_error_instead_of_empty_history() {
    let fx = fixture("missing-backfill", 4, 16);
    std::fs::remove_file(&fx.path).unwrap();
    assert!(parse_page_before(&fx.registry, "window-test", &fx.path, fx.total).is_err());
    assert_eq!(fx.reads.load(Ordering::Relaxed), 0);
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
