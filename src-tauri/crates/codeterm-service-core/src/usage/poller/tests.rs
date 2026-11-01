use super::{
    evaluate_exhaustion_debounce, failure_kind_for, row_reset_due, snapshot_is_exhausted,
    ExhaustionSignal, EXHAUSTION_CONFIRM_THRESHOLD,
};
use crate::agent::usage_parser::UsageSnapshot;
use crate::plugin::provider::CAPABILITY_DENIED_ERROR;
use crate::usage::probe::ProbeError;
use crate::usage::store::{UsageFailureKind, UsageSnapshotRow};

#[test]
fn probe_admission_bounds_refresh_bursts_and_failed_reset_retries() {
    let now = std::time::Instant::now();
    let mut state = super::AccountState::default();
    assert!(state.start_probe(now));
    assert!(!state.start_probe(now + std::time::Duration::from_secs(120)));
    state.in_flight = false;
    state.consecutive_failures = 1;
    for seconds in [0, 1, 60, 119] {
        assert!(!state.start_probe(now + std::time::Duration::from_secs(seconds)));
        assert_eq!(state.last_probe_at, Some(now));
    }
    assert!(state.start_probe(now + std::time::Duration::from_secs(120)));
    assert_eq!(
        state.last_probe_at,
        Some(now + std::time::Duration::from_secs(120))
    );
}

#[test]
fn probe_admission_keeps_accounts_independent_and_bounds_wake_bursts() {
    let now = std::time::Instant::now();
    let mut first = super::AccountState::default();
    let mut second = super::AccountState::default();
    assert!(first.start_probe(now));
    assert!(second.start_probe(now));
    first.in_flight = false;
    let wake = now + std::time::Duration::from_secs(86_400);
    assert!(first.start_probe(wake));
    first.in_flight = false;
    assert!(!first.start_probe(wake));
}

fn snap_with(session: Option<f32>, weekly: Option<f32>, spend: Option<f64>) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty("claude", "", 0);
    snap.session_pct = session;
    snap.weekly_pct = weekly;
    snap.spend_remaining_cents = spend;
    snap
}

#[test]
fn provider_snapshot_mapping_persists_every_meter_and_reset() {
    let data_dir = tempfile::tempdir().unwrap();
    let store = crate::usage::UsageStore::open(data_dir.path()).unwrap();
    let snapshot = UsageSnapshot {
        usage_details: None,
        provider: "fixture".into(),
        billing_mode: codeterm_usage::contract::BillingMode::Subscription,
        limits_reported: true,
        account_id: Some("account-a".into()),
        captured_at_ms: 1_783_601_000_000,
        session_pct: Some(12.5),
        session_resets_at_ms: Some(1_783_609_680_000),
        weekly_pct: Some(27.5),
        weekly_resets_at_ms: Some(1_784_139_960_000),
        spend_cents: Some(1743.5),
        spend_limit_cents: Some(2000.0),
        spend_remaining_cents: Some(256.5),
        spend_resets_at_ms: Some(1_785_542_400_000),
        raw_text: "provider fixture".into(),
    };
    let sample = super::usage_sample(&snapshot);
    assert_eq!(sample.raw_text, snapshot.raw_text);
    let id = store.insert_snapshot(&sample).unwrap();
    drop(store);
    let reopened = crate::usage::UsageStore::open(data_dir.path()).unwrap();
    let row = reopened
        .get_latest("fixture", Some("account-a"))
        .unwrap()
        .unwrap();
    assert_eq!(row.id, id);
    assert!(row.parsed_ok);
    assert_eq!(row.provider, snapshot.provider);
    assert_eq!(row.account_id, snapshot.account_id);
    assert_eq!(row.captured_at, snapshot.captured_at_ms);
    assert_eq!(row.billing_mode, snapshot.billing_mode);
    assert_eq!(row.limits_reported, snapshot.limits_reported);
    assert_eq!(row.session_pct, snapshot.session_pct);
    assert_eq!(row.session_resets_at, snapshot.session_resets_at_ms);
    assert_eq!(row.weekly_pct, snapshot.weekly_pct);
    assert_eq!(row.weekly_resets_at, snapshot.weekly_resets_at_ms);
    assert_eq!(row.spend_cents, snapshot.spend_cents);
    assert_eq!(row.spend_limit_cents, snapshot.spend_limit_cents);
    assert_eq!(row.spend_remaining_cents, snapshot.spend_remaining_cents);
    assert_eq!(row.spend_resets_at, snapshot.spend_resets_at_ms);
    assert!(reopened
        .get_latest("fixture", Some("account-b"))
        .unwrap()
        .is_none());
}

#[test]
fn snapshot_exhaustion_reads_every_meter() {
    assert!(snapshot_is_exhausted(&snap_with(Some(100.0), None, None)));
    assert!(snapshot_is_exhausted(&snap_with(None, Some(100.0), None)));
    assert!(snapshot_is_exhausted(&snap_with(None, None, Some(0.0))));
    assert!(snapshot_is_exhausted(&snap_with(None, None, Some(-5.0))));
    assert!(!snapshot_is_exhausted(&snap_with(
        Some(99.9),
        Some(99.9),
        Some(1.0)
    )));
}

/// A SINGLE >=100 sample must interrupt no worker: the debounce advances the
/// streak but never crosses the threshold on one reading.
#[test]
fn single_exhausted_sample_does_not_fire() {
    let (mut streak, mut fired) = (0, false);
    assert_eq!(
        evaluate_exhaustion_debounce(&mut streak, &mut fired, true, EXHAUSTION_CONFIRM_THRESHOLD),
        None
    );
    assert_eq!(streak, 1);
    assert!(!fired);
}

/// The confirming sequence fires exactly once, and stays quiet on further
/// exhausted readings until a healthy reading resets the debounce.
#[test]
fn sustained_exhaustion_fires_exactly_once() {
    let (mut streak, mut fired) = (0, false);
    let mut signals = Vec::new();
    for _ in 0..(EXHAUSTION_CONFIRM_THRESHOLD + 2) {
        signals.push(evaluate_exhaustion_debounce(
            &mut streak,
            &mut fired,
            true,
            EXHAUSTION_CONFIRM_THRESHOLD,
        ));
    }
    let confirmations = signals
        .iter()
        .filter(|s| **s == Some(ExhaustionSignal::Confirmed))
        .count();
    assert_eq!(
        confirmations, 1,
        "confirmed exactly once across a sustained run"
    );
    assert!(fired);
}

/// A sub-100 reading resets the debounce and (once fired) yields recovery.
#[test]
fn healthy_reading_resets_and_recovers() {
    let (mut streak, mut fired) = (0, false);
    for _ in 0..EXHAUSTION_CONFIRM_THRESHOLD {
        evaluate_exhaustion_debounce(&mut streak, &mut fired, true, EXHAUSTION_CONFIRM_THRESHOLD);
    }
    assert!(fired);
    assert_eq!(
        evaluate_exhaustion_debounce(&mut streak, &mut fired, false, EXHAUSTION_CONFIRM_THRESHOLD),
        Some(ExhaustionSignal::Recovered)
    );
    assert_eq!(streak, 0);
    assert!(!fired);
}

/// A spurious >=100 blip followed by a normal reading interrupts nothing:
/// the streak never reaches the threshold and no signal is emitted.
#[test]
fn blip_then_recovery_never_fires() {
    let (mut streak, mut fired) = (0, false);
    assert_eq!(
        evaluate_exhaustion_debounce(&mut streak, &mut fired, true, EXHAUSTION_CONFIRM_THRESHOLD),
        None
    );
    assert_eq!(
        evaluate_exhaustion_debounce(&mut streak, &mut fired, false, EXHAUSTION_CONFIRM_THRESHOLD),
        None
    );
    assert_eq!(streak, 0);
    assert!(!fired);
}

fn row(session: Option<i64>, weekly: Option<i64>) -> UsageSnapshotRow {
    UsageSnapshotRow {
        usage_details: None,
        billing_mode: Default::default(),
        limits_reported: false,
        id: 1,
        provider: "claude".into(),
        account_id: None,
        captured_at: 0,
        parsed_ok: true,
        session_pct: Some(90.0),
        session_resets_at: session,
        weekly_pct: Some(50.0),
        weekly_resets_at: weekly,
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

fn row_with_spend(spend: Option<i64>) -> UsageSnapshotRow {
    UsageSnapshotRow {
        usage_details: None,
        spend_resets_at: spend,
        ..row(None, None)
    }
}

#[test]
fn reset_not_due_when_timers_in_future() {
    let now = 1_000_000;
    assert!(!row_reset_due(&row(Some(now + 1), Some(now + 5)), now));
}

#[test]
fn reset_due_when_session_timer_passed() {
    let now = 1_000_000;
    assert!(row_reset_due(&row(Some(now - 1), Some(now + 5)), now));
}

#[test]
fn reset_due_when_weekly_timer_passed() {
    let now = 1_000_000;
    assert!(row_reset_due(&row(Some(now + 5), Some(now)), now));
}

#[test]
fn reset_due_when_spend_timer_passed() {
    let now = 1_000_000;
    assert!(row_reset_due(&row_with_spend(Some(now)), now));
}

#[test]
fn reset_not_due_when_no_timers() {
    assert!(!row_reset_due(&row(None, None), 1_000_000));
}

/// Falsification oracle for "the usage poller reads the meter through a
/// path that consumes the meter". A provider with no quota-free usage
/// source has only the metered agent-spawn path left, so scheduling it at
/// all is the defect — a visible tab does not make the read free.
#[test]
fn provider_without_quota_free_source_is_never_scheduled() {
    assert!(
        !super::provider_pollable(false, false),
        "no quota-free usage source → must not be scheduled, even with a visible Tab"
    );
    assert!(
        !super::provider_pollable(true, false),
        "no quota-free usage source and no Tab → must not be scheduled"
    );
}

#[test]
fn fetch_provider_polls_without_a_tab() {
    assert!(
        super::provider_pollable(true, true),
        "fetch-based provider needs no terminal → poll even with no Tab"
    );
    assert!(
        super::provider_pollable(false, true),
        "Tab present → still poll"
    );
}

/// A confined plugin's dead probe and a real upstream fetch failure look
/// identical from the outside; only the marker separates them, and if it
/// stops doing so a lost capability goes back to reading as a network blip.
#[test]
fn a_capability_denial_is_classified_apart_from_an_upstream_fetch_failure() {
    let denied = ProbeError::FetchFailed(format!(
        "{CAPABILITY_DENIED_ERROR} fetchUsage returned null"
    ));
    assert_eq!(
        failure_kind_for(&denied),
        UsageFailureKind::CapabilityDenied,
        "a denied capability must name itself"
    );

    let upstream = ProbeError::FetchFailed("request timed out".to_string());
    assert_eq!(failure_kind_for(&upstream), UsageFailureKind::FetchError);
    assert_ne!(failure_kind_for(&upstream), failure_kind_for(&denied));
}
