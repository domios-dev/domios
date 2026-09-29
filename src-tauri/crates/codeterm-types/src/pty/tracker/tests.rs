use super::*;

#[test]
fn test_unknown_tab_is_dead() {
    let tracker = TabStatusTracker::new();
    assert_eq!(tracker.get_status("nonexistent"), TabStatus::Dead);
}

#[test]
fn test_update_output_sets_working() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Working);
}

#[test]
fn test_set_waiting() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.set_waiting("pty-1");
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Waiting);
}

#[test]
fn test_set_dead() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.set_dead("pty-1");
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Dead);
}

#[test]
fn provider_error_status_notifies_waiters_and_is_visible_without_output_history() {
    let mut tracker = TabStatusTracker::new();
    let signal = tracker.arm_status_change_signal("pty-1");
    let generation = signal.generation();

    tracker.set_error("pty-1");

    assert_eq!(tracker.get_status("pty-1"), TabStatus::Error);
    assert!(signal.wait_after(generation, Duration::ZERO));
}

#[test]
fn provider_error_status_survives_repaint_until_provider_starts_another_turn() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.set_error("pty-1");

    tracker.update_output("pty-1", [2u8; 32]);
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Error);
    tracker.set_idle("pty-1");
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Error);

    tracker.set_working("pty-1");
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Working);
}

#[test]
fn test_check_idle_not_timed_out() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.check_idle("pty-1", Duration::from_secs(60));
    // Should still be working (not enough time passed)
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Working);
}

#[test]
fn test_check_idle_timed_out() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.check_idle("pty-1", Duration::from_millis(0));
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Idle);
}

#[test]
fn test_update_output_resets_to_working() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.check_idle("pty-1", Duration::from_millis(0));
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Idle);

    tracker.update_output("pty-1", [2u8; 32]);
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Working);
}

#[test]
fn test_remove() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.remove("pty-1");
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Dead);
}

#[test]
fn test_same_screen_hash_does_not_reset_timer() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.check_idle("pty-1", Duration::from_millis(0));
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Idle);
    tracker.update_output("pty-1", [1u8; 32]);
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Idle);
}

#[test]
fn test_get_status_if_tracked() {
    let mut tracker = TabStatusTracker::new();
    assert_eq!(tracker.get_status_if_tracked("pty-1"), None);
    tracker.update_output("pty-1", [1u8; 32]);
    assert_eq!(
        tracker.get_status_if_tracked("pty-1"),
        Some(TabStatus::Working)
    );
}

#[test]
fn test_all_ids() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.update_output("pty-2", [2u8; 32]);
    let ids = tracker.all_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"pty-1".to_string()));
    assert!(ids.contains(&"pty-2".to_string()));
}

#[test]
fn test_update_output_returns_true_on_content_change() {
    let mut tracker = TabStatusTracker::new();
    assert!(tracker.update_output("pty-1", [1u8; 32]));
    assert!(tracker.update_output("pty-1", [2u8; 32]));
}

#[test]
fn test_update_output_returns_false_on_same_hash() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    assert!(!tracker.update_output("pty-1", [1u8; 32]));
}

#[test]
fn test_waiting_not_affected_by_idle_check() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    tracker.set_waiting("pty-1");
    // check_idle should not affect Waiting status (only transitions Working->Idle)
    tracker.check_idle("pty-1", Duration::from_millis(0));
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Waiting);
}

#[test]
fn status_signal_advances_only_on_status_transition() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    let signal = tracker.arm_status_change_signal("pty-1");
    let working = signal.generation();

    tracker.update_output("pty-1", [1u8; 32]);
    assert_eq!(signal.generation(), working);

    tracker.check_idle("pty-1", Duration::ZERO);
    assert_ne!(signal.generation(), working);
    assert_eq!(tracker.get_status("pty-1"), TabStatus::Idle);
}

#[test]
fn tab_stop_waits_for_provider_status_event() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-stop", [1u8; 32]);
    let signal = tracker.arm_terminal_event_signal("pty-stop");
    let generation = signal.generation();

    tracker.check_idle("pty-stop", Duration::ZERO);
    assert!(!signal.wait_after(generation, Duration::ZERO));
    assert!(!tracker.get_status("pty-stop").has_active_turn());

    tracker.set_idle("pty-stop");
    tracker.notify_terminal_event("pty-stop");

    assert!(signal.wait_after(generation, Duration::ZERO));
    assert!(!tracker.get_status("pty-stop").has_active_turn());
}

#[test]
fn removing_pty_closes_status_signal() {
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("pty-1", [1u8; 32]);
    let signal = tracker.arm_status_change_signal("pty-1");
    let generation = signal.generation();

    tracker.remove("pty-1");
    assert!(!signal.wait_after(generation, Duration::ZERO));
}
