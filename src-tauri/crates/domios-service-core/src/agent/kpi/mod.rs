//! KPI choke-point instrumentation. Observation only: nothing here may alter delivery,
//! lifecycle, report or nudge behaviour, and every writer is gated on `kpi.enabled`.

pub mod attribution;
mod rebuild;
pub mod recorder;
pub mod usage;

pub use attribution::{deliver_event_kind, KpiAttribution};
pub use recorder::KpiRecorder;
pub use usage::UsageSource;

#[cfg(test)]
mod dedupe_tests;
#[cfg(test)]
mod tests;
