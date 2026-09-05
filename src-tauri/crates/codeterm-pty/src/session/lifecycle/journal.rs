use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::record::{
    JournalRecord, JournalSeq, LifecycleCheckpoint, LifecycleEntry, LifecycleEpoch, LifecycleKey,
    LifecycleTransition,
};

pub const JOURNAL_FILE_NAME: &str = "tab-lifecycle.jsonl";

/// Records after which the next append compacts, bounding the file.
pub const COMPACT_AT_RECORDS: usize = 4096;

/// How the loader classified the end of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailStatus {
    Intact,
    /// A line failed to parse or broke the monotonic sequence. Everything from
    /// this position on is refused rather than reordered or repaired.
    Refused {
        line: usize,
        reason: String,
    },
}

impl TailStatus {
    pub fn is_intact(&self) -> bool {
        matches!(self, Self::Intact)
    }
}

/// Result of replaying the journal file. Records are in journal order.
#[derive(Debug, Clone)]
pub struct JournalLoad {
    pub records: Vec<JournalRecord>,
    pub tail: TailStatus,
}

impl JournalLoad {
    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
            tail: TailStatus::Intact,
        }
    }

    pub fn last_seq(&self) -> JournalSeq {
        self.records.last().map(JournalRecord::seq).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy)]
struct WriteCursor {
    last_seq: JournalSeq,
    records: usize,
}

/// Append-only tab lifecycle journal. One file, one monotonic sequence.
pub struct TabLifecycleJournal {
    path: PathBuf,
    cursor: Mutex<WriteCursor>,
}

static SHARED: OnceLock<TabLifecycleJournal> = OnceLock::new();

impl TabLifecycleJournal {
    /// Process-wide journal in the same directory the session store writes to.
    pub fn shared() -> &'static Self {
        SHARED.get_or_init(|| Self::for_dir(codeterm_paths::data_dir().join("sessions")))
    }

    pub fn for_dir(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(JOURNAL_FILE_NAME);
        let load = read_journal(&path);
        Self {
            path,
            cursor: Mutex::new(WriteCursor {
                last_seq: load.last_seq(),
                records: load.records.len(),
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one committed transition. The caller owns the mutation; the
    /// journal never infers one.
    pub fn append(
        &self,
        key: LifecycleKey,
        transition: LifecycleTransition,
    ) -> Result<JournalSeq, String> {
        let mut cursor = self
            .cursor
            .lock()
            .map_err(|_| "lifecycle journal lock poisoned".to_string())?;
        let seq = cursor.last_seq.saturating_add(1);
        let record = JournalRecord::Transition {
            entry: LifecycleEntry {
                seq,
                key,
                transition,
                recorded_at: super::stamp::now_utc(),
            },
        };
        append_line(&self.path, &record)?;
        cursor.last_seq = seq;
        cursor.records += 1;
        let unbounded = cursor.records >= COMPACT_AT_RECORDS;
        drop(cursor);
        if unbounded {
            if let Err(error) = self.compact() {
                log::warn!("[tab-lifecycle] compaction failed: {error}");
            }
        }
        Ok(seq)
    }

    pub fn load(&self) -> JournalLoad {
        read_journal(&self.path)
    }

    /// Rewrite as one checkpoint per tab plus the transitions no checkpoint
    /// covers. Replay reaches the same checkpoints as the full history.
    pub fn compact(&self) -> Result<usize, String> {
        let mut cursor = self
            .cursor
            .lock()
            .map_err(|_| "lifecycle journal lock poisoned".to_string())?;
        let load = read_journal(&self.path);
        if let TailStatus::Refused { line, reason } = &load.tail {
            return Err(format!(
                "refusing to compact a journal whose tail is unreadable (line {line}: {reason})"
            ));
        }
        let compacted = super::reduce::compact_records(&load.records);
        let written = compacted.len();
        write_all_records(&self.path, &compacted)?;
        cursor.records = written;
        Ok(written)
    }
}

fn append_line(path: &Path, record: &JournalRecord) -> Result<(), String> {
    let mut line = serde_json::to_string(record).map_err(|error| error.to_string())?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("append {}: {error}", path.display()))
}

fn write_all_records(path: &Path, records: &[JournalRecord]) -> Result<(), String> {
    let mut body = String::new();
    for record in records {
        body.push_str(&serde_json::to_string(record).map_err(|error| error.to_string())?);
        body.push('\n');
    }
    codeterm_util::atomic_write::write_atomic(path, body.as_bytes())
        .map_err(|error| format!("replace {}: {error}", path.display()))
}

/// A malformed or out-of-order line refuses itself and everything after it: a
/// truncated tail must never silently become history.
fn read_journal(path: &Path) -> JournalLoad {
    let Ok(body) = fs::read_to_string(path) else {
        return JournalLoad::empty();
    };
    let mut records: Vec<JournalRecord> = Vec::new();
    let mut last_seq: JournalSeq = 0;
    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: JournalRecord = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(error) => {
                return JournalLoad {
                    records,
                    tail: TailStatus::Refused {
                        line: index + 1,
                        reason: format!("unparseable record: {error}"),
                    },
                }
            }
        };
        let seq = record.seq();
        if seq <= last_seq {
            return JournalLoad {
                records,
                tail: TailStatus::Refused {
                    line: index + 1,
                    reason: format!("sequence {seq} does not advance past {last_seq}"),
                },
            };
        }
        last_seq = seq;
        records.push(record);
    }
    JournalLoad {
        records,
        tail: TailStatus::Intact,
    }
}

/// Next epoch for a tab, derived from its own journal history only.
pub fn next_epoch_for(load: &JournalLoad, tab_id: &str) -> LifecycleEpoch {
    load.records
        .iter()
        .filter(|record| record.tab_id() == tab_id)
        .map(|record| match record {
            JournalRecord::Transition { entry } => entry.key.epoch,
            JournalRecord::Checkpoint { checkpoint } => checkpoint.key.epoch,
        })
        .max()
        .map(LifecycleEpoch::next)
        .unwrap_or_else(LifecycleEpoch::first)
}

/// Checkpoints carried by an already-loaded journal, newest sealed operation
/// per tab.
pub fn checkpoints(load: &JournalLoad) -> Vec<LifecycleCheckpoint> {
    super::reduce::sealed_checkpoints(&load.records)
}

pub fn checkpoint_history(load: &JournalLoad, tab_id: &str) -> Vec<LifecycleCheckpoint> {
    super::reduce::checkpoint_history(&load.records, tab_id)
}
