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
    let session_weekly_remaining = (100.0 - consumed).clamp(0.0, 100.0);
    // A merely low or degenerate remaining/limit pair is a soft budget signal and must never gate a healthy session/weekly window; only session/weekly headroom does that.
    if spend_genuinely_exhausted(snapshot) {
        0.0
    } else {
        session_weekly_remaining
    }
}

fn spend_genuinely_exhausted(snapshot: &UsageSnapshotRow) -> bool {
    snapshot
        .spend_remaining_cents
        .zip(snapshot.spend_limit_cents)
        .is_some_and(|(remaining, limit)| {
            remaining.is_finite() && limit.is_finite() && limit > 0.0 && remaining <= 0.0
        })
}

#[cfg(test)]
mod allowance_tests {
    use super::*;

    fn snapshot() -> UsageSnapshotRow {
        UsageSnapshotRow {
            id: 0,
            provider: "claude".into(),
            account_id: None,
            captured_at: 0,
            parsed_ok: true,
            session_pct: None,
            session_resets_at: None,
            weekly_pct: None,
            weekly_resets_at: None,
            spend_cents: None,
            spend_limit_cents: None,
            spend_remaining_cents: None,
            spend_resets_at: None,
            failure_reason: None,
            probe_command: None,
            raw_excerpt: None,
            debug_json: None,
        }
    }

    const RESERVE: f32 = 5.0;

    #[test]
    fn healthy_session_weekly_without_spend_dispatches() {
        let mut snap = snapshot();
        snap.session_pct = Some(8.0);
        snap.weekly_pct = Some(45.0);
        let remaining = allowance_remaining_pct(&snap);
        assert!((remaining - 55.0).abs() < f32::EPSILON, "got {remaining}");
        assert!(remaining >= RESERVE);
    }

    #[test]
    fn near_zero_spend_pair_does_not_block_healthy_session_weekly() {
        let mut snap = snapshot();
        snap.session_pct = Some(8.0);
        snap.weekly_pct = Some(45.0);
        snap.spend_limit_cents = Some(2000.0);
        snap.spend_remaining_cents = Some(20.0);
        let remaining = allowance_remaining_pct(&snap);
        assert!(
            remaining >= RESERVE,
            "near-zero spend must not gate: {remaining}"
        );
        assert!((remaining - 55.0).abs() < f32::EPSILON);
    }

    #[test]
    fn degenerate_spend_pair_falls_back_to_session_weekly() {
        for (remaining_cents, limit_cents) in [
            (Some(0.0), Some(0.0)),
            (None, Some(2000.0)),
            (Some(20.0), None),
            (Some(f32::NAN as f64), Some(2000.0)),
        ] {
            let mut snap = snapshot();
            snap.session_pct = Some(8.0);
            snap.weekly_pct = Some(45.0);
            snap.spend_remaining_cents = remaining_cents;
            snap.spend_limit_cents = limit_cents;
            let remaining = allowance_remaining_pct(&snap);
            assert!(
                remaining >= RESERVE,
                "degenerate pair must not gate: {remaining}"
            );
        }
    }

    #[test]
    fn genuine_spend_exhaustion_still_blocks() {
        let mut snap = snapshot();
        snap.session_pct = Some(8.0);
        snap.weekly_pct = Some(45.0);
        snap.spend_limit_cents = Some(2000.0);
        snap.spend_remaining_cents = Some(0.0);
        let remaining = allowance_remaining_pct(&snap);
        assert!(
            remaining < RESERVE,
            "true spend exhaustion must gate: {remaining}"
        );
    }

    #[test]
    fn near_exhausted_session_still_blocks() {
        let mut snap = snapshot();
        snap.session_pct = Some(97.0);
        let remaining = allowance_remaining_pct(&snap);
        assert!(
            remaining < RESERVE,
            "session>100-reserve must gate: {remaining}"
        );
        assert!((remaining - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn near_exhausted_weekly_still_blocks() {
        let mut snap = snapshot();
        snap.session_pct = Some(10.0);
        snap.weekly_pct = Some(96.0);
        let remaining = allowance_remaining_pct(&snap);
        assert!(
            remaining < RESERVE,
            "weekly>100-reserve must gate: {remaining}"
        );
    }
}
