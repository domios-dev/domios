//! Durable tab lifecycle audit: one append-only journal, and a restore that
//! accepts only an exactly matching checkpoint. See `docs/TAB-LIFECYCLE.md`.

mod gate;
mod inspect;
mod journal;
mod record;
mod recovery;
mod reduce;
mod stamp;

#[cfg(test)]
mod tests;

pub use gate::{gate_session_snapshot, TabRecoveryOutcome};
pub use inspect::inspect_tab_lifecycle;
pub use journal::{
    checkpoints, next_epoch_for, JournalLoad, TabLifecycleJournal, TailStatus, COMPACT_AT_RECORDS,
    JOURNAL_FILE_NAME,
};
pub use record::{
    CommittedAuthority, JournalRecord, JournalSeq, LifecycleCheckpoint, LifecycleEntry,
    LifecycleEpoch, LifecycleKey, LifecycleTransition,
};
pub use recovery::{authorize, Lineage, RecoveryRefusal, RecoveryVerdict, SnapshotAuthorityClaim};
