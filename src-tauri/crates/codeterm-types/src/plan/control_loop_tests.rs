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
