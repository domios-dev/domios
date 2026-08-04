//! Durable tab lifecycle audit. One append-only journal keyed by tab id plus a
//! lifecycle epoch and operation id; a restart may restore authority only from
//! a complete committed checkpoint whose lineage is exactly the snapshot's.
//! Contract and file format: `docs/TAB-LIFECYCLE.md`.

mod gate;
mod journal;
mod recovery;
mod record;
mod reduce;
mod stamp;

#[cfg(test)]
mod tests;

pub use gate::{gate_session_snapshot, SnapshotGate, TabRecoveryOutcome};
pub use journal::{
    checkpoints, coverage_id, next_epoch_for, JournalLoad, TabLifecycleJournal, TailStatus,
    COMPACT_AT_RECORDS, JOURNAL_FILE_NAME,
};
pub use recovery::{authorize, RecoveryRefusal, RecoveryVerdict, SnapshotAuthorityClaim};
pub use record::{
    CommittedAuthority, CoverageRecord, JournalRecord, JournalSeq, LifecycleCheckpoint,
    LifecycleEntry, LifecycleEpoch, LifecycleKey, LifecycleTransition,
};
