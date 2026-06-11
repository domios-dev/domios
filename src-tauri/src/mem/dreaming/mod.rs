//! Dreaming consolidation — promotes high-signal mem_items based on access
//! frequency and importance. Light + Deep phases run as a cron-spawned agent
//! by default; can also be invoked manually via `mem dream now`.
//!
//! In Phase 3.4 dreaming runs locally (no LLM call) — pure SQL signals over
//! the existing `accessed_at` and `importance` columns. The full vision in
//! mem-architecture.md (Light → Deep → REM with token budgets) is layered on
//! top of this baseline in later phases as needed.

use serde::{Deserialize, Serialize};

pub mod recluster;
mod rem;
mod schedule;

#[cfg(test)]
mod phases;
#[cfg(test)]
mod tests;

pub use rem::{prepare_rem_data, spawn_rem_worker};
pub use schedule::{
    ensure_dreaming_cron, remove_legacy_librarian_cron, DREAMING_CRON_NAME, LIBRARIAN_CRON_NAME,
};

#[cfg(test)]
pub(crate) use phases::{deep_phase, light_phase, normalize_phase};
#[cfg(test)]
pub(crate) use rem::{DREAMING_TELEGRAM_STEP_DISABLED, DREAMING_TELEGRAM_STEP_ENABLED};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamReport {
    pub considered: u64,
    pub promoted: u64,
    pub dropped: u64,
    pub phase: String,
}
