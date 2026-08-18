//! Durable tab lifecycle telemetry: one append-only journal, written by the
//! code that commits each mutation. Restore never consults it — session
//! restore is unconditional. See `docs/TAB-LIFECYCLE.md`.

mod inspect;
mod journal;
mod record;
mod reduce;
mod stamp;

#[cfg(test)]
mod tests;

pub use inspect::inspect_tab_lifecycle;
pub use journal::{
    checkpoints, next_epoch_for, JournalLoad, TabLifecycleJournal, TailStatus, COMPACT_AT_RECORDS,
    JOURNAL_FILE_NAME,
};
pub use record::{
    CommittedAuthority, JournalRecord, JournalSeq, LifecycleCheckpoint, LifecycleEntry,
    LifecycleEpoch, LifecycleKey, LifecycleTransition,
};
