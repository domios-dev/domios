use std::collections::HashMap;
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
    /// Records dropped because a later record reused their sequence number.
    pub duplicate_seqs: usize,
}

impl JournalLoad {
    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
            tail: TailStatus::Intact,
            duplicate_seqs: 0,
        }
    }

    pub fn last_seq(&self) -> JournalSeq {
        self.records.last().map(JournalRecord::seq).unwrap_or(0)
    }
}

#[derive(Debug)]
struct WriteCursor {
    last_seq: JournalSeq,
    records: usize,
    /// Record count at the last refused compaction; the next attempt waits for growth past it.
    refused_at_records: Option<usize>,
    last_epoch_by_tab: HashMap<String, LifecycleEpoch>,
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
        if let Err(error) = quarantine_refused_tail(&path, &load) {
            log::warn!("[tab-lifecycle] refused tail kept in place: {error}");
        }
        let mut last_epoch_by_tab = HashMap::new();
        for record in &load.records {
            let epoch = record_epoch(record);
            last_epoch_by_tab
                .entry(record.tab_id().to_string())
                .and_modify(|last: &mut LifecycleEpoch| *last = (*last).max(epoch))
                .or_insert(epoch);
        }
        Self {
            path,
            cursor: Mutex::new(WriteCursor {
                last_seq: load.last_seq(),
                records: load.records.len(),
                refused_at_records: None,
                last_epoch_by_tab,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_epoch_for_tab(&self, tab_id: &str) -> LifecycleEpoch {
        self.cursor
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .last_epoch_by_tab
            .get(tab_id)
            .copied()
            .map(LifecycleEpoch::next)
            .unwrap_or_else(LifecycleEpoch::first)
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
        let tab_id = key.tab_id.clone();
        let epoch = key.epoch;
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
        cursor
            .last_epoch_by_tab
            .entry(tab_id)
            .and_modify(|last| *last = (*last).max(epoch))
            .or_insert(epoch);
        let unbounded = cursor.records >= COMPACT_AT_RECORDS
            && cursor.refused_at_records.map_or(true, |refused| {
                cursor.records >= refused + COMPACT_AT_RECORDS
            });
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
        quarantine_refused_tail(&self.path, &load)?;
        let compacted = super::reduce::compact_records(&load.records);
        let written = compacted.len();
        write_all_records(&self.path, &compacted)?;
        if load.duplicate_seqs > 0 {
            log::warn!(
                "[tab-lifecycle] repaired {} duplicate sequence number(s) while compacting",
                load.duplicate_seqs
            );
        }
        cursor.records = written;
        cursor.refused_at_records = None;
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

/// Sidecar that receives a refused tail, verbatim and never replayed.
pub fn refused_tail_path(path: &Path) -> PathBuf {
    path.with_extension("refused.jsonl")
}

/// Moves a refused tail aside so later appends land behind an intact prefix
/// instead of behind a line the loader stops at forever.
fn quarantine_refused_tail(path: &Path, load: &JournalLoad) -> Result<(), String> {
    let TailStatus::Refused { line, reason } = &load.tail else {
        return Ok(());
    };
    let body =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut refused: String = body
        .lines()
        .skip(line.saturating_sub(1))
        .flat_map(|text| [text, "\n"])
        .collect();
    if refused.is_empty() {
        refused.push('\n');
    }
    let sidecar = refused_tail_path(path);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&sidecar)
        .and_then(|mut file| file.write_all(refused.as_bytes()))
        .map_err(|error| format!("append {}: {error}", sidecar.display()))?;
    write_all_records(path, &load.records)?;
    log::warn!(
        "[tab-lifecycle] quarantined journal tail from line {line} ({reason}) into {}",
        sidecar.display()
    );
    Ok(())
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
    let mut duplicate_seqs = 0usize;
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
                    duplicate_seqs,
                }
            }
        };
        let seq = record.seq();
        if seq == last_seq && !records.is_empty() {
            records.pop();
            duplicate_seqs += 1;
            records.push(record);
            continue;
        }
        if seq < last_seq {
            return JournalLoad {
                records,
                tail: TailStatus::Refused {
                    line: index + 1,
                    reason: format!("sequence {seq} does not advance past {last_seq}"),
                },
                duplicate_seqs,
            };
        }
        last_seq = seq;
        records.push(record);
    }
    JournalLoad {
        records,
        tail: TailStatus::Intact,
        duplicate_seqs,
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

fn record_epoch(record: &JournalRecord) -> LifecycleEpoch {
    match record {
        JournalRecord::Transition { entry } => entry.key.epoch,
        JournalRecord::Checkpoint { checkpoint } => checkpoint.key.epoch,
    }
}

/// Checkpoints carried by an already-loaded journal, newest sealed operation
/// per tab.
pub fn checkpoints(load: &JournalLoad) -> Vec<LifecycleCheckpoint> {
    super::reduce::sealed_checkpoints(&load.records)
}

pub fn checkpoint_history(load: &JournalLoad, tab_id: &str) -> Vec<LifecycleCheckpoint> {
    super::reduce::checkpoint_history(&load.records, tab_id)
}
