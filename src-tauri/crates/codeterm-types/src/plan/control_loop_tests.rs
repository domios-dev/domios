use super::*;

#[test]
fn question_quota_tracks_receipts_and_preserves_uncertainty() {
    let mut run = CuratorRun::default();
    assert!(run.reserve_question());
    assert!(!run.asked);
    assert!(!run.reserve_question());
    run.settle_question(CuratorMessageReceipt::NothingWritten);
    assert!(run.reserve_question());
    run.settle_question(CuratorMessageReceipt::Uncertain);
    assert!(!run.asked);
    assert!(!run.reserve_question());
    run.settle_question(CuratorMessageReceipt::Accepted);
    assert!(run.asked);
    assert!(!run.reserve_question());
}

#[test]
fn refusal_history_keeps_the_latest_twenty() {
    let mut run = CuratorRun::default();
    for number in 0..25 {
        run.refuse_action(number.to_string());
    }
    assert_eq!(run.refused_actions.len(), 20);
    assert_eq!(run.refused_actions.first().unwrap(), "5");
    assert_eq!(run.refused_actions.last().unwrap(), "24");
}

#[test]
fn a_team_window_narrows_or_clamps_inside_the_host_bounds() {
    let host = CuratorWindow::HOST;
    assert_eq!(CuratorWindow::within_host(45 * 60, 2 * 60 * 60), host);
    let narrow = CuratorWindow::within_host(60 * 60, 90 * 60);
    assert_eq!((narrow.min_secs, narrow.max_secs), (3600, 5400));
    assert_eq!(CuratorWindow::within_host(60, 24 * 60 * 60), host);
    assert_eq!(narrow.delay(None), 3600);
    assert_eq!(narrow.delay(Some(u64::MAX)), 5400);
    assert_eq!(narrow.delay(Some(1)), 3600);
    // A stored window outside the host bounds is clamped again when used.
    let stored = CuratorWindow {
        min_secs: 1,
        max_secs: u64::MAX,
    };
    assert_eq!(stored.delay(None), host.min_secs);
    assert_eq!(stored.delay(Some(u64::MAX)), host.max_secs);

    let mut state = ControlLoop {
        curator_window: Some(narrow),
        ..Default::default()
    };
    state.defer(100, Some(u64::MAX));
    assert_eq!(state.next_at, Some(100 + 5400 * 1000));
    assert_eq!(ControlLoop::default().delay(None), host.min_secs);
}

#[test]
fn owner_off_overrides_failure_fallback_and_deferral() {
    let mut state = ControlLoop::default();
    state.fallback_reason = Some(CuratorFailure::ProviderUnavailable);
    state.defer(100, Some(u64::MAX));
    assert_eq!(state.next_at, Some(7_200_100));
    assert!(state.owner_heartbeat());
    state.mode = ControlLoopMode::Off;
    state.defer(200, None);
    assert_eq!(state.next_at, None);
    assert!(!state.owner_heartbeat());
    state.mode = ControlLoopMode::Curator;
    state.defer(u64::MAX - 1, Some(u64::MAX));
    assert_eq!(state.next_at, Some(u64::MAX));
}
