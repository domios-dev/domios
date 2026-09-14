//! Incremental provider-usage ingestion. Every source is read forward-only from the
//! cursor left by the previous pass, so a re-ingest of an unchanged session is a stat
//! plus a zero-byte read — never a whole-file re-parse.
//!
//! Nothing here branches on a provider id: the source kind is decided by the sqlite
//! magic header, and each record by its own field shape (see [`records`]).

pub mod records;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use records::{parse_record, UsageRecord};

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
}

/// Forward-only position in a source: byte offset for line logs, last rowid for sqlite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cursor {
    offset: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageBatch {
    pub records: Vec<UsageRecord>,
    /// How far this pass moved the cursor. Those bytes are committed past once the
    /// batch lands, so a pass that consumes them and records nothing is worth naming.
    pub consumed: u64,
    /// Lines the parser could not read at all. Their bytes are committed past too,
    /// which makes them loss rather than delay, so they are never swallowed.
    pub unreadable_lines: u64,
}

/// Category totals over a batch — the shape the fixture tests and phase 4 aggregate on.
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
}

#[derive(Default)]
pub struct UsageIngestor {
    cursors: HashMap<PathBuf, Cursor>,
}

impl UsageIngestor {
    /// Returns only records appended since the previous call for this source.
    pub fn ingest(&mut self, source: &UsageSource) -> Result<UsageBatch, String> {
        let cursor = self.cursors.get(&source.path).copied().unwrap_or_default();
        let (batch, next) = if is_sqlite(&source.path)? {
            read_sqlite_since(&source.path, cursor)?
        } else {
            read_lines_since(&source.path, cursor)?
        };
        self.cursors.insert(source.path.clone(), next);
        Ok(batch)
    }

    /// Current forward-only read position for a source, if it has been ingested at
    /// least once this run (or seeded from a persisted watermark).
    pub fn cursor_offset(&self, source: &UsageSource) -> Option<u64> {
        self.cursors.get(&source.path).map(|c| c.offset)
    }

    /// Resumes a source from a previously persisted watermark instead of byte/rowid
    /// zero. A no-op if this source already has a cursor for the current run.
    pub fn seed_cursor(&mut self, source: &UsageSource, offset: u64) {
        self.cursors
            .entry(source.path.clone())
            .or_insert(Cursor { offset });
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
/// truncated, so the cursor resets rather than skipping the new content.
fn read_lines_since(path: &Path, cursor: Cursor) -> Result<(UsageBatch, Cursor), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {:?}: {}", path, e))?;
    let len = file
        .metadata()
        .map_err(|e| format!("stat {:?}: {}", path, e))?
        .len();
    let start = if len < cursor.offset {
        0
    } else {
        cursor.offset
    };
    if len == start {
        return Ok((UsageBatch::default(), Cursor { offset: len }));
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("seek {:?}: {}", path, e))?;

    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut unreadable_lines = 0u64;
    let mut consumed = start;
    let mut line = String::new();
    loop {
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
        consumed += read as u64;
        match parse_json_line(&line) {
            ParsedLine::Usage(record) => records.push(record),
            ParsedLine::NoUsage => {}
            ParsedLine::Unreadable => unreadable_lines += 1,
        }
    }
    Ok((
        UsageBatch {
            records,
            consumed: consumed.saturating_sub(start),
            unreadable_lines,
        },
        Cursor { offset: consumed },
    ))
}

/// Three outcomes, kept distinct on purpose: a usage record, a line that is valid
/// and simply carries none, and a line the parser could not read at all. The
/// cursor commits past all three, so only the last one is loss.
enum ParsedLine {
    Usage(UsageRecord),
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

/// sqlite sources advance by rowid — the same forward-only contract as a byte offset.
fn read_sqlite_since(path: &Path, cursor: Cursor) -> Result<(UsageBatch, Cursor), String> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("open sqlite {:?}: {}", path, e))?;
    let mut stmt = conn
        .prepare("SELECT rowid, data FROM message WHERE rowid > ?1 ORDER BY rowid ASC")
        .map_err(|e| format!("prepare message scan: {}", e))?;
    let rows = stmt
        .query_map([cursor.offset as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query messages: {}", e))?;

    let mut records = Vec::new();
    let mut unreadable_lines = 0u64;
    let mut last_rowid = cursor.offset;
    for row in rows.flatten() {
        let (rowid, data) = row;
        last_rowid = rowid as u64;
        match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(value) => {
                if let Some(record) = parse_record(&value) {
                    records.push(record);
                }
            }
            Err(_) => unreadable_lines += 1,
        }
    }
    Ok((
        UsageBatch {
            records,
            consumed: last_rowid.saturating_sub(cursor.offset),
            unreadable_lines,
        },
        Cursor { offset: last_rowid },
    ))
}

#[cfg(test)]
mod tests;
