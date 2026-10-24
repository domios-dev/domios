use super::*;

#[test]
fn only_the_accepted_turn_satisfies_the_start_wait() {
    let starts = TurnStarts::default();
    starts.observe("previous");
    let before = starts.sequence();
    assert!(!starts.started(before, None));
    assert!(!starts.started(before, Some("previous")));
    starts.observe("unrelated");
    assert!(!starts.started(before, Some("accepted")));
    starts.observe("accepted");
    assert!(starts.started(before, Some("accepted")));
    assert!(starts.started(before, None));
    let sequence = starts.sequence();
    starts.observe("accepted");
    assert_eq!(starts.sequence(), sequence);
}

#[tokio::test]
async fn event_before_wait_is_retained_without_a_settle_delay() {
    let starts = TurnStarts::default();
    let before = starts.sequence();
    starts.observe("accepted");
    starts.wait(before, Some("accepted")).await;
}
