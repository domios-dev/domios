//! Durable tab lifecycle telemetry: one append-only journal, written by the
//! code that commits each mutation. Normal restore does not use it as
//! authority. See `docs/TAB-LIFECYCLE.md`.

mod inspect;
mod journal;
mod record;
mod reduce;
mod stamp;

#[cfg(test)]
mod tests;

pub use inspect::inspect_tab_lifecycle;
pub use journal::{
    checkpoint_history, checkpoints, next_epoch_for, JournalLoad, TabLifecycleJournal, TailStatus,
    COMPACT_AT_RECORDS, JOURNAL_FILE_NAME,
};
pub use record::{
    AgentTypeWriteAudit, AgentTypeWriteOrigin, CommittedAuthority, JournalRecord, JournalSeq,
    LifecycleCheckpoint, LifecycleCheckpointHistoryEntry, LifecycleEntry, LifecycleEpoch,
    LifecycleKey, LifecycleTransition,
};
pub use reduce::PRIOR_CHECKPOINT_LIMIT;
