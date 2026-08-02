pub mod poller;
pub mod probe;
pub mod projection;
pub mod store;
mod suspicious;

pub use poller::{TabActivity, UsagePoller};
pub use probe::{ProbeError, UsageProbe, UsageProbeOutcome};
pub use projection::{ProjectionResponse, SessionProjection, WeeklyProjection};
pub use store::{DailyRollup, UsageSnapshotRow, UsageStore, UsageStoreHandle};

pub(crate) fn allowance_remaining_pct(snapshot: &UsageSnapshotRow) -> f32 {
    let consumed = snapshot
        .session_pct
        .into_iter()
        .chain(snapshot.weekly_pct)
        .fold(0.0_f32, f32::max);
    snapshot
        .spend_remaining_cents
        .zip(snapshot.spend_limit_cents)
        .filter(|(_, limit)| *limit > 0.0)
        .map(|(remaining, limit)| ((remaining / limit) * 100.0) as f32)
        .map(|spend_remaining| (100.0 - consumed).min(spend_remaining))
        .unwrap_or(100.0 - consumed)
        .clamp(0.0, 100.0)
}
