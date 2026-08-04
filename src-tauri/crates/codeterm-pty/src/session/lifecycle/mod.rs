//! Durable tab lifecycle audit: one append-only journal, and a restore that
//! accepts only an exactly matching checkpoint. See `docs/TAB-LIFECYCLE.md`.

mod gate;
mod journal;
mod recovery;
mod record;
mod reduce;
mod stamp;

#[cfg(test)]
mod tests;

pub use gate::{gate_session_snapshot, SnapshotCoverage, SnapshotGate, TabRecoveryOutcome};
pub use journal::{
    checkpoints, coverage_id, next_epoch_for, JournalLoad, TabLifecycleJournal, TailStatus,
    COMPACT_AT_RECORDS, JOURNAL_FILE_NAME,
};
pub use recovery::{authorize, RecoveryRefusal, RecoveryVerdict, SnapshotAuthorityClaim};
pub use record::{
    CommittedAuthority, CoverageRecord, JournalRecord, JournalSeq, LifecycleCheckpoint,
    LifecycleEntry, LifecycleEpoch, LifecycleKey, LifecycleTransition,
};
