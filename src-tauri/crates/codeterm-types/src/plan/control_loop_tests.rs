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
