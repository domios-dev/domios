use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TAB: AtomicU64 = AtomicU64::new(0);

fn tab() -> String {
    format!(
        "tab-state-pure-{}",
        NEXT_TAB.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn tab_state_single_strong_hint_non_actionable() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::WeakHint
    );
    assert!(!snapshot_episode(&tab).signaled);
}

#[test]
fn tab_state_two_consecutive_strong_acts_once_per_label() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::StrongAction
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::None
    );
}

#[test]
fn tab_state_dedup_per_tab_label_episode() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::StrongAction
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::None
    );
    screen_changed(&tab, "launch-1", 1);
    note_output(&tab);
    assert!(!screen_changed(&tab, "launch-1", 1));
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9),
        SignalAction::None
    );
    assert!(screen_changed(&tab, "launch-1", 2));
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::StrongAction
    );
}

#[test]
fn supporter_first_opinion_no_default_request_cap_per_episode() {
    let tab = tab();
    let ids: BTreeSet<_> = (0..601)
        .map(|_| {
            claim_observation(&tab, "launch-1", ObservationEvent::WorkerIdle)
                .unwrap()
                .observation_id
        })
        .collect();
    assert_eq!(ids.len(), 601);
}

#[test]
fn tab_state_verdict_discarded_across_new_launch() {
    let tab = tab();
    let first = claim_observation(&tab, "launch-1", ObservationEvent::WorkerIdle).unwrap();
    assert_eq!(
        apply_observed_verdict(&tab, "launch-1", &first, "error_loop", 0.9,),
        Some(SignalAction::WeakHint)
    );
    note_output(&tab);
    assert!(permit_is_current(&tab, "launch-1", &first));

    let next = claim_observation(&tab, "launch-2", ObservationEvent::TabExit).unwrap();
    assert_ne!(first.episode, next.episode);
    assert_eq!(
        apply_observed_verdict(&tab, "launch-2", &first, "error_loop", 0.9,),
        None
    );
    assert_eq!(
        apply_observed_verdict(&tab, "launch-2", &next, "error_loop", 0.9,),
        Some(SignalAction::WeakHint)
    );
}

#[test]
fn supporter_first_opinion_old_launch_verdict_preserves_current_episode() {
    let tab_id = tab();
    let old = claim_observation(&tab_id, "launch-1", ObservationEvent::WorkerIdle).unwrap();
    let current = claim_observation(&tab_id, "launch-2", ObservationEvent::WorkerIdle).unwrap();
    let before = snapshot_episode(&tab_id);

    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-1", &old, "error_loop", 0.95),
        None
    );
    assert_eq!(snapshot_episode(&tab_id), before);
    assert!(permit_is_current(&tab_id, "launch-2", &current));
    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-2", &current, "error_loop", 0.95),
        Some(SignalAction::WeakHint)
    );
}

#[test]
fn supporter_first_opinion_unclaimed_verdict_does_not_create_episode() {
    let tab_id = tab();
    let permit = ObservationPermit {
        episode: 1,
        observation_id: 1,
        spawn_claim: None,
    };
    let before = snapshot_episode(&tab_id);

    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-1", &permit, "error_loop", 0.95),
        None
    );
    assert_eq!(snapshot_episode(&tab_id), before);
}

#[test]
fn supporter_first_opinion_duplicate_observation_cannot_supply_agreement() {
    let tab_id = tab();
    let first = claim_observation(&tab_id, "launch-1", ObservationEvent::WorkerIdle).unwrap();
    let second =
        claim_observation(&tab_id, "launch-1", ObservationEvent::StartupEscalation).unwrap();

    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-1", &first, "error_loop", 0.95),
        Some(SignalAction::WeakHint)
    );
    let before = snapshot_episode(&tab_id);
    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-1", &first, "error_loop", 0.95),
        None
    );
    assert_eq!(snapshot_episode(&tab_id), before);
    assert_ne!(first.observation_id, second.observation_id);
    assert_eq!(
        apply_observed_verdict(&tab_id, "launch-1", &second, "error_loop", 0.95),
        Some(SignalAction::StrongAction)
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
fn tab_state_launch_key_is_stable_across_binding() {
    let tab_id = tab();
    let before_binding = launch_episode_key(&tab_id, Some("pty-launch"), None);
    let after_binding =
        launch_episode_key(&tab_id, Some("pty-launch"), Some(std::time::Instant::now()));
    let permit = claim_observation(&tab_id, &before_binding, ObservationEvent::SpawnReady).unwrap();

    assert_eq!(before_binding, after_binding);
    assert!(permit_is_current(&tab_id, &after_binding, &permit));
    assert_ne!(
        after_binding,
        launch_episode_key(&tab_id, Some("next-pty-launch"), None)
    );
}

#[test]
fn tab_state_spawn_claim_survives_signal_episode_bump() {
    let tab_id = tab();
    let permit = claim_observation(&tab_id, "launch-1", ObservationEvent::SpawnReady).unwrap();
    for _ in 0..3 {
        note_output(&tab_id);
    }

    assert!(permit_is_current_for_launch(&tab_id, "launch-1", &permit));
    assert!(permit_is_current(&tab_id, "launch-1", &permit));
    assert!(!permit_is_current_for_launch(
        &tab_id,
        "next-launch",
        &permit
    ));

    let bumped = tab();
    let permit = claim_observation(&bumped, "launch-1", ObservationEvent::SpawnReady).unwrap();
    apply_verdict(&bumped, "launch-1", "error_loop", 0.9);
    apply_verdict(&bumped, "launch-1", "error_loop", 0.9);
    screen_changed(&bumped, "launch-1", 1);
    note_output(&bumped);
    assert_eq!(snapshot_episode(&bumped).episode, permit.episode);
    screen_changed(&bumped, "launch-1", 2);
    assert_eq!(snapshot_episode(&bumped).episode, permit.episode + 1);
    assert!(!permit_is_current(&bumped, "launch-1", &permit));
    assert!(snapshot_episode(&bumped).spawn_observed);
    assert!(permit_is_current_for_launch(&bumped, "launch-1", &permit));
}

#[test]
fn tab_state_disagreeing_strong_no_action() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.9,),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "model_unavailable_or_quota", 0.9,),
        SignalAction::None
    );
    assert!(!snapshot_episode(&tab).signaled);
}

#[test]
fn tab_state_weak_band_one_hint_per_episode() {
    let tab = tab();
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.7,),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.7,),
        SignalAction::None
    );
    note_output(&tab);
    assert_eq!(
        apply_verdict(&tab, "launch-1", "error_loop", 0.7,),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab, "launch-2", "error_loop", 0.7,),
        SignalAction::WeakHint
    );
    assert!(!snapshot_episode(&tab).signaled);
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

#[test]
fn tab_state_other_interrupts_consecutive_strong_agreement() {
    let tab_id = tab();
    assert_eq!(
        apply_verdict(&tab_id, "launch", "error_loop", 0.9),
        SignalAction::WeakHint
    );
    assert_eq!(
        apply_verdict(&tab_id, "launch", super::super::OTHER, 0.99),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab_id, "launch", "error_loop", 0.9),
        SignalAction::None
    );
    assert_eq!(
        apply_verdict(&tab_id, "launch", "error_loop", 0.9),
        SignalAction::StrongAction
    );
}

#[test]
fn supporter_first_opinion_captured_exit_cannot_reset_new_launch() {
    let tab = tab();
    let old = claim_observation(&tab, "nonce-1", ObservationEvent::WorkerIdle).unwrap();
    assert!(audit_permit(&tab, "nonce-1", next_observation_id()).is_some());
    claim_observation(&tab, "nonce-2", ObservationEvent::WorkerIdle).unwrap();
    let current = snapshot_episode(&tab);
    assert!(audit_permit(&tab, "nonce-1", old.observation_id).is_none());
    assert_eq!(snapshot_episode(&tab), current);
}
