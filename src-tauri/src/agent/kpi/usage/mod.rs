//! Incremental provider-usage ingestion. Every source is read forward-only from the
//! cursor left by the previous pass, so a re-ingest of an unchanged session is a stat
//! plus a zero-byte read — never a whole-file re-parse.
//!
//! Nothing here branches on a provider id: the source kind is decided by the sqlite
//! magic header, and each record by its own field shape (see [`records`]).

pub mod records;
pub mod responses;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use records::{parse_record, ParsedUsage};
use responses::{CollectedUsage, ResponseCollector};
pub use responses::{CumulativeBaseline, UsageRecord};

use crate::agent::kpi_ledger::{TokenCounts, UsageCursorState};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// A provider-written session log to ingest. `path` comes from the provider capability
/// (`AgentProvider::session_file_path`), never from a hardcoded per-provider layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSource {
    pub path: PathBuf,
}

impl UsageSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn key(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// Forward-only position in a source (byte offset for line logs, last rowid for sqlite)
/// plus the running totals already counted up to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageCursor {
    pub offset: u64,
    pub baseline: CumulativeBaseline,
}

impl UsageCursor {
    pub fn from_persisted(state: UsageCursorState) -> Self {
        Self {
            offset: state.offset,
            baseline: CumulativeBaseline::resumed(state.offset, state.cumulative),
        }
    }

    pub fn persisted(self) -> UsageCursorState {
        UsageCursorState {
            offset: self.offset,
            cumulative: self.baseline.totals(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageBatch {
    pub records: Vec<UsageRecord>,
    /// Cumulative lines that repeated totals already counted, as each line reported them.
    pub superseded: Vec<(u64, TokenCounts)>,
    /// How far this pass moved the cursor. Those bytes are committed past once the
    /// batch lands, so a pass that consumes them and records nothing is worth naming.
    pub consumed: u64,
    /// Lines the parser could not read at all. Their bytes are committed past too,
    /// which makes them loss rather than delay, so they are never swallowed.
    pub unreadable_lines: u64,
}

/// Category totals over a batch — the shape the fixture tests aggregate on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    pub compactions: u64,
}

impl UsageBatch {
    pub fn totals(&self) -> UsageTotals {
        let mut totals = UsageTotals::default();
        for record in &self.records {
            match record.tokens {
                Some(t) => {
                    totals.input_tokens += t.input;
                    totals.uncached_input_tokens += t.uncached_input;
                    totals.output_tokens += t.output;
                }
                None => totals.compactions += 1,
            }
        }
        totals
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Moved the cursor without recording anything. Ordinary for a stretch of log
    /// carrying no usage; the caller reports it rather than hiding it.
    pub fn consumed_without_recording(&self) -> bool {
        self.records.is_empty() && self.consumed > 0
    }

    fn collected(collected: CollectedUsage, consumed: u64, unreadable_lines: u64) -> Self {
        Self {
            records: collected.records,
            superseded: collected.superseded,
            consumed,
            unreadable_lines,
        }
    }
}

#[derive(Default)]
pub struct UsageIngestor {
    cursors: HashMap<PathBuf, UsageCursor>,
}

impl UsageIngestor {
    /// Returns only records appended since the previous call for this source.
    pub fn ingest(&mut self, source: &UsageSource) -> Result<UsageBatch, String> {
        let cursor = self.cursors.get(&source.path).copied().unwrap_or_default();
        let (batch, next) = read_source(source, cursor, None)?;
        self.cursors.insert(source.path.clone(), next);
        Ok(batch)
    }

    /// Re-reads a source from its start through `until`, the watermark it was committed at.
    pub fn read_through(
        source: &UsageSource,
        until: u64,
    ) -> Result<(UsageBatch, UsageCursor), String> {
        read_source(source, UsageCursor::default(), Some(until))
    }

    pub fn cursor(&self, source: &UsageSource) -> Option<UsageCursor> {
        self.cursors.get(&source.path).copied()
    }

    /// Current forward-only read position for a source, if it has been ingested at
    /// least once this run (or seeded from a persisted watermark).
    pub fn cursor_offset(&self, source: &UsageSource) -> Option<u64> {
        self.cursor(source).map(|c| c.offset)
    }

    /// Resumes a source from a previously persisted watermark instead of byte/rowid
    /// zero. A no-op if this source already has a cursor for the current run.
    pub fn seed_cursor(&mut self, source: &UsageSource, cursor: UsageCursor) {
        self.cursors.entry(source.path.clone()).or_insert(cursor);
    }

    /// Drops the in-process cursor so the next pass resumes from the persisted one.
    pub fn forget(&mut self, source: &UsageSource) {
        self.cursors.remove(&source.path);
    }
}

fn read_source(
    source: &UsageSource,
    cursor: UsageCursor,
    until: Option<u64>,
) -> Result<(UsageBatch, UsageCursor), String> {
    if is_sqlite(&source.path)? {
        read_sqlite_since(source, cursor, until)
    } else {
        read_lines_since(source, cursor, until)
    }
}

fn is_sqlite(path: &Path) -> Result<bool, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {:?}: {}", path, e))?;
    let mut header = [0u8; SQLITE_MAGIC.len()];
    match file.read_exact(&mut header) {
        Ok(()) => Ok(&header == SQLITE_MAGIC),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(format!("read header {:?}: {}", path, e)),
    }
}

/// Tail-parse from `cursor.offset`. A file shorter than the cursor was rotated or
/// truncated, so the cursor and its running totals reset rather than skipping content.
fn read_lines_since(
    source: &UsageSource,
    cursor: UsageCursor,
    until: Option<u64>,
) -> Result<(UsageBatch, UsageCursor), String> {
    let path = &source.path;
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {:?}: {}", path, e))?;
    let len = file
        .metadata()
        .map_err(|e| format!("stat {:?}: {}", path, e))?
        .len();
    let start = if len < cursor.offset {
        UsageCursor::default()
    } else {
        cursor
    };
    if len == start.offset {
        return Ok((UsageBatch::default(), start));
    }
    file.seek(SeekFrom::Start(start.offset))
        .map_err(|e| format!("seek {:?}: {}", path, e))?;

    let source_key = source.key();
    let mut collector = ResponseCollector::new(&source_key, start.baseline);
    let mut reader = BufReader::new(file);
    let mut unreadable_lines = 0u64;
    let mut consumed = start.offset;
    let mut line = String::new();
    loop {
        if until.is_some_and(|end| consumed >= end) {
            break;
        }
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("read {:?}: {}", path, e))?;
        if read == 0 {
            break;
        }
        // A trailing fragment without a newline is a half-written line: leave the
        // cursor before it so the next pass re-reads it whole.
        if !line.ends_with('\n') {
            break;
        }
        let position = consumed;
        consumed += read as u64;
        match parse_json_line(&line) {
            ParsedLine::Usage(parsed) => collector.push(parsed, position),
            ParsedLine::NoUsage => {}
            ParsedLine::Unreadable => unreadable_lines += 1,
        }
    }
    let collected = collector.finish();
    let next = UsageCursor {
        offset: consumed,
        baseline: collected.baseline,
    };
    let batch = UsageBatch::collected(collected, consumed - start.offset, unreadable_lines);
    Ok((batch, next))
}

/// Three outcomes, kept distinct on purpose: a usage record, a line that is valid
/// and simply carries none, and a line the parser could not read at all. The
/// cursor commits past all three, so only the last one is loss.
enum ParsedLine {
    Usage(ParsedUsage),
    NoUsage,
    Unreadable,
}

fn parse_json_line(line: &str) -> ParsedLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedLine::NoUsage;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return ParsedLine::Unreadable;
    };
    match parse_record(&value) {
        Some(record) => ParsedLine::Usage(record),
        None => ParsedLine::NoUsage,
    }
}

/// A message row's own primary key names its response when the JSON blob omits one.
fn with_row_identity(parsed: ParsedUsage, row_id: Option<String>) -> ParsedUsage {
    match parsed {
        ParsedUsage::Response {
            timestamp,
            response_id: None,
            tokens,
        } => ParsedUsage::Response {
            timestamp,
            response_id: row_id,
            tokens,
        },
        other => other,
    }
}

/// sqlite sources advance by rowid — the same forward-only contract as a byte offset.
fn read_sqlite_since(
    source: &UsageSource,
    cursor: UsageCursor,
    until: Option<u64>,
) -> Result<(UsageBatch, UsageCursor), String> {
    let path = &source.path;
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("open sqlite {:?}: {}", path, e))?;
    let mut stmt = conn
        .prepare(
            "SELECT rowid, id, data FROM message WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid ASC",
        )
        .map_err(|e| format!("prepare message scan: {}", e))?;
    let upper = until.map_or(i64::MAX, |end| i64::try_from(end).unwrap_or(i64::MAX));
    let rows = stmt
        .query_map([cursor.offset as i64, upper], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("query messages: {}", e))?;

    let source_key = source.key();
    let mut collector = ResponseCollector::new(&source_key, cursor.baseline);
    let mut unreadable_lines = 0u64;
    let mut last_rowid = cursor.offset;
    for (rowid, row_id, data) in rows.flatten() {
        last_rowid = rowid as u64;
        match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(value) => {
                if let Some(parsed) = parse_record(&value) {
                    collector.push(with_row_identity(parsed, row_id), last_rowid);
                }
            }
            Err(_) => unreadable_lines += 1,
        }
    }
    let collected = collector.finish();
    let next = UsageCursor {
        offset: last_rowid,
        baseline: collected.baseline,
    };
    let batch = UsageBatch::collected(
        collected,
        last_rowid.saturating_sub(cursor.offset),
        unreadable_lines,
    );
    Ok((batch, next))
}

#[cfg(test)]
mod dedupe_tests;
#[cfg(test)]
mod tests;
