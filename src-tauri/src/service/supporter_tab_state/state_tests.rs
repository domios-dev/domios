use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TAB: AtomicU64 = AtomicU64::new(0);

fn tab() -> String {
    format!("tab-state-pure-{}", NEXT_TAB.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn tab_state_band_mapping() {
    assert_eq!(confidence_band(0.80), ConfidenceBand::Strong);
    assert_eq!(confidence_band(0.79), ConfidenceBand::Weak);
    assert_eq!(confidence_band(0.60), ConfidenceBand::Weak);
    assert_eq!(confidence_band(0.59), ConfidenceBand::Ignored);
}

#[test]
fn tab_state_single_strong_is_only_weak_hint() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, true),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, true),
        SignalAction::StrongAction
    );
}

#[test]
fn tab_state_two_consecutive_strong_acts_once_per_label() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::StrongAction
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::None
    );
}

#[test]
fn tab_state_dedup_per_tab_label_episode() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::StrongAction
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::None
    );
    note_output(&tab);
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9, true, false),
        SignalAction::StrongAction
    );
}

#[test]
fn tab_state_call_cap_per_episode() {
    let tab = tab();
    for _ in 0..CALL_CAP_PER_EPISODE {
        assert!(claim_observation(&tab, "launch-1", ObservationEvent::WorkerIdle).is_ok());
    }
    assert_eq!(
        claim_observation(&tab, "launch-1", ObservationEvent::WorkerIdle),
        Err(PermitError::CallCapReached)
    );
}

#[test]
fn tab_state_stale_observation_cannot_signal_a_new_episode() {
    let tab = tab();
    let first = claim_observation(&tab, "launch-1", ObservationEvent::WorkerIdle).unwrap();
    assert_eq!(
        apply_observed_verdict(
            &tab,
            "launch-1",
            &first,
            "error_loop",
            0.9,
            true,
            true,
        ),
        Some(SignalAction::WeakHint)
    );
    note_output(&tab);
    assert_eq!(
        apply_observed_verdict(
            &tab,
            "launch-1",
            &first,
            "error_loop",
            0.9,
            true,
            true,
        ),
        None
    );

    let next = claim_observation(&tab, "launch-1", ObservationEvent::TabExit).unwrap();
    assert_ne!(first.episode, next.episode);
    assert_eq!(
        apply_observed_verdict(
            &tab,
            "launch-1",
            &next,
            "error_loop",
            0.9,
            true,
            true,
        ),
        Some(SignalAction::WeakHint)
    );
}

#[test]
fn tab_state_spawn_health_fires_once_ready_or_deadline() {
    let tab = tab();
    assert!(claim_observation(&tab, "launch-1", ObservationEvent::SpawnReady).is_ok());
    assert_eq!(
        claim_observation(&tab, "launch-1", ObservationEvent::SpawnDeadline),
        Err(PermitError::SpawnAlreadyObserved)
    );
}

#[test]
fn tab_state_busy_event_is_once_until_new_output() {
    let tab = tab();
    assert!(claim_observation(&tab, "launch-1", ObservationEvent::BusyNoOutput).is_ok());
    assert_eq!(
        claim_observation(&tab, "launch-1", ObservationEvent::BusyNoOutput),
        Err(PermitError::BusyAlreadyObserved)
    );
    note_output(&tab);
    assert!(claim_observation(&tab, "launch-1", ObservationEvent::BusyNoOutput).is_ok());
}
