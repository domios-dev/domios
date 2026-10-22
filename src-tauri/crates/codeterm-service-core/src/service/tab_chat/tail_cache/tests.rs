use std::cell::Cell;

use super::*;

const W: u64 = SNAPSHOT_WINDOW_BYTES;

struct FakeRecord {
    start: u64,
    len: u64,
    rows: usize,
}

/// In-memory transcript: complete records plus an optional partial tail.
struct FakeFile {
    records: Vec<FakeRecord>,
    partial: u64,
    generation: u64,
    parsed: Cell<u64>,
}

impl FakeFile {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            partial: 0,
            generation: 0,
            parsed: Cell::new(0),
        }
    }

    fn complete_end(&self) -> u64 {
        self.records
            .last()
            .map_or(0, |record| record.start + record.len)
    }

    fn push(&mut self, len: u64, rows: usize) {
        let start = self.complete_end();
        self.records.push(FakeRecord { start, len, rows });
        self.partial = 0;
    }

    fn rows_of(&self, records: &[&FakeRecord]) -> (Vec<ChatMsg>, Vec<u64>) {
        let mut messages = Vec::new();
        let mut starts = Vec::new();
        for record in records {
            for row in 0..record.rows {
                messages.push(ChatMsg {
                    id: format!("rec-{}-{row}", record.start),
                    content: format!("{}:{row}", record.start),
                    ..Default::default()
                });
                starts.push(record.start);
            }
        }
        (messages, starts)
    }
}

impl TailPort for FakeFile {
    fn length(&self) -> Result<u64, String> {
        Ok(self.complete_end() + self.partial)
    }

    fn probe(&self, end: u64) -> Result<u64, String> {
        if end > self.complete_end() + self.partial {
            return Err("probe exceeds transcript length".into());
        }
        Ok(self.generation.wrapping_mul(1_000_003).wrapping_add(end))
    }

    fn window(&self) -> Result<(TailRows, u64), String> {
        let end = self.complete_end();
        let page_start = self
            .records
            .iter()
            .find(|record| end - record.start <= W)
            .or(self.records.last())
            .map_or(end, |record| record.start);
        let in_page: Vec<&FakeRecord> = self
            .records
            .iter()
            .filter(|record| record.start >= page_start)
            .collect();
        let counts: Vec<usize> = in_page
            .iter()
            .filter(|record| record.rows > 0)
            .map(|record| record.rows)
            .collect();
        let first_group = retained_group_start(&counts, SNAPSHOT_ROW_LIMIT);
        let kept: Vec<&FakeRecord> = in_page
            .into_iter()
            .filter(|record| record.rows > 0)
            .skip(first_group)
            .collect();
        let (messages, record_starts) = self.rows_of(&kept);
        self.parsed.set(self.parsed.get() + end - page_start);
        Ok((
            TailRows {
                start_offset: record_starts.first().copied().unwrap_or(page_start),
                end_offset: end,
                messages,
                record_starts,
            },
            end - page_start,
        ))
    }

    fn append(&self, from: u64, until: u64) -> Result<(TailRows, u64), String> {
        let fresh: Vec<&FakeRecord> = self
            .records
            .iter()
            .filter(|record| record.start >= from && record.start + record.len <= until)
            .collect();
        let end = fresh
            .last()
            .map_or(from, |record| record.start + record.len);
        let (messages, record_starts) = self.rows_of(&fresh);
        self.parsed.set(self.parsed.get() + end - from);
        Ok((
            TailRows {
                start_offset: from,
                end_offset: end,
                messages,
                record_starts,
            },
            end - from,
        ))
    }
}

fn key(name: &str) -> TailKey {
    TailKey {
        provider_id: "fixture".into(),
        path: PathBuf::from(name),
    }
}

fn fresh(file: &FakeFile) -> TailRows {
    TailCache::default()
        .read(&key("fresh"), file)
        .map(|read| read.rows.as_ref().clone())
        .unwrap()
}

#[test]
fn plan_reuses_unchanged_appends_growth_and_rebuilds_rewrites() {
    assert_eq!(plan(100, 100, 100, true), TailPlan::Reuse);
    assert_eq!(plan(110, 100, 110, true), TailPlan::Reuse);
    assert_eq!(plan(100, 100, 150, true), TailPlan::Append { from: 100 });
    assert_eq!(plan(100, 100, 150, false), TailPlan::Rebuild);
    assert_eq!(plan(100, 100, 90, true), TailPlan::Rebuild);
    assert_eq!(plan(100, 100, 100 + W, true), TailPlan::Rebuild);
    assert_eq!(plan(100, 100, 99 + W, true), TailPlan::Append { from: 100 });
}

#[test]
fn reopening_an_unchanged_transcript_parses_no_transcript_bytes() {
    let mut file = FakeFile::new();
    for index in 0..200 {
        file.push(30_000, index % 5);
    }
    let cache = TailCache::default();
    let first = cache.read(&key("a"), &file).unwrap();
    assert_eq!(first.plan, TailPlan::Rebuild);
    let parsed_after_open = file.parsed.get();
    for _ in 0..100 {
        let again = cache.read(&key("a"), &file).unwrap();
        assert_eq!(again.plan, TailPlan::Reuse);
        assert!(again.bytes_read <= PROBE_BYTES);
        assert!(Arc::ptr_eq(&again.rows, &first.rows));
    }
    assert_eq!(file.parsed.get(), parsed_after_open);
}

#[test]
fn appends_read_only_new_bytes_and_match_a_fresh_window() {
    let mut file = FakeFile::new();
    for index in 0..10 {
        file.push(20_000, index % 3 + 1);
    }
    let cache = TailCache::default();
    cache.read(&key("live"), &file).unwrap();
    for step in 0..400u64 {
        let rows = match step % 7 {
            0 => 0,
            1 => 12,
            _ => (step % 4) as usize + 1,
        };
        let len = 5_000 + (step % 11) * 3_000;
        file.push(len, rows);
        let before = file.parsed.get();
        let read = cache.read(&key("live"), &file).unwrap();
        assert_eq!(
            read.plan,
            TailPlan::Append {
                from: file.complete_end() - len
            }
        );
        assert_eq!(file.parsed.get() - before, len);
        let expected = fresh(&file);
        assert_eq!(read.rows.messages, expected.messages, "step {step}");
        assert_eq!(read.rows.record_starts, expected.record_starts);
        assert_eq!(read.rows.end_offset, expected.end_offset);
        if !expected.messages.is_empty() {
            assert_eq!(read.rows.start_offset, expected.start_offset);
        }
        assert!(read.rows.messages.len() <= SNAPSHOT_ROW_LIMIT);
    }
}

#[test]
fn a_partial_trailing_record_is_held_until_complete() {
    let mut file = FakeFile::new();
    file.push(1_000, 2);
    let cache = TailCache::default();
    let opened = cache.read(&key("p"), &file).unwrap();
    file.partial = 400;
    let held = cache.read(&key("p"), &file).unwrap();
    assert_eq!(held.plan, TailPlan::Append { from: 1_000 });
    assert_eq!(held.rows.messages, opened.rows.messages);
    assert_eq!(held.rows.end_offset, 1_000);
    let unchanged = cache.read(&key("p"), &file).unwrap();
    assert_eq!(unchanged.plan, TailPlan::Reuse);
    file.push(700, 1);
    let completed = cache.read(&key("p"), &file).unwrap();
    assert_eq!(completed.plan, TailPlan::Append { from: 1_000 });
    assert_eq!(completed.rows.messages.len(), 3);
}

#[test]
fn rewrites_truncations_and_oversized_appends_rebuild() {
    let mut file = FakeFile::new();
    for _ in 0..4 {
        file.push(10_000, 1);
    }
    let cache = TailCache::default();
    cache.read(&key("r"), &file).unwrap();
    file.generation += 1;
    assert_eq!(
        cache.read(&key("r"), &file).unwrap().plan,
        TailPlan::Rebuild
    );
    file.records.pop();
    assert_eq!(
        cache.read(&key("r"), &file).unwrap().plan,
        TailPlan::Rebuild
    );
    file.push(W + 1, 1);
    let rebuilt = cache.read(&key("r"), &file).unwrap();
    assert_eq!(rebuilt.plan, TailPlan::Rebuild);
    assert_eq!(rebuilt.rows.messages, fresh(&file).messages);
}

#[test]
fn entries_are_bounded_and_least_recent_evicts_first() {
    let mut file = FakeFile::new();
    file.push(100, 1);
    let cache = TailCache::with_limits(2, MAX_BYTES);
    cache.read(&key("one"), &file).unwrap();
    cache.read(&key("two"), &file).unwrap();
    cache.read(&key("one"), &file).unwrap();
    cache.read(&key("three"), &file).unwrap();
    assert_eq!(cache.len(), 2);
    assert_eq!(
        cache.read(&key("one"), &file).unwrap().plan,
        TailPlan::Reuse
    );
    assert_eq!(
        cache.read(&key("two"), &file).unwrap().plan,
        TailPlan::Rebuild
    );
    let unbounded = TailCache::with_limits(4, 1);
    unbounded.read(&key("big"), &file).unwrap();
    assert_eq!(unbounded.len(), 0);
}

#[test]
fn incremental_charges_equal_a_rebuilt_window_after_eviction() {
    let mut file = FakeFile::new();
    let cache = TailCache::default();
    let key = key("charged");
    for _ in 0..400 {
        file.push(20_000, 3);
        cache.read(&key, &file).unwrap();
        let fresh = TailCache::default();
        fresh.read(&key, &file).unwrap();
        let entries = cache.entries.lock();
        let rebuilt = fresh.entries.lock();
        assert_eq!(entries[0].bytes, rebuilt[0].bytes);
        assert_eq!(entries[0].charges, rebuilt[0].charges);
    }
}

#[test]
fn unavailable_transcript_returns_an_error_instead_of_an_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let cache = TailCache::default();
    assert!(cache
        .jsonl_tail(
            &registry,
            "fixture",
            &dir.path().join("missing.jsonl"),
            "test"
        )
        .is_err());
    assert!(cache
        .jsonl_tail(&registry, "fixture", dir.path(), "test")
        .is_err());
    assert_eq!(cache.len(), 0);
}

#[test]
fn appending_keeps_retained_message_allocations() {
    let mut file = FakeFile::new();
    file.push(1000, 1);
    let cache = TailCache::default();
    let key = key("owned");
    let first = cache.read(&key, &file).unwrap();
    let retained = first.rows.messages[0].content.as_ptr();
    drop(first);
    file.push(1000, 1);
    let appended = cache.read(&key, &file).unwrap();
    assert_eq!(appended.rows.messages[0].content.as_ptr(), retained);
    assert_eq!(appended.rows.messages.len(), 2);
}
