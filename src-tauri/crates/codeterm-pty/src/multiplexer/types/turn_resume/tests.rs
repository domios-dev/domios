use super::*;

const FIRST: &str = "start-1";
const SECOND: &str = "start-2";
const THIRD: &str = "start-3";

fn working(session: &str, start: &str) -> Option<TurnResumeMark> {
    let mut mark = None;
    assert!(observe_turn(&mut mark, Some(session), start));
    mark
}

#[test]
fn working_turn_records_its_session_and_start() {
    let mark = working("session-a", FIRST).expect("working mark");

    assert_eq!(mark.session_id, "session-a");
    assert_eq!(mark.start_epoch, FIRST);
    assert!(mark.settled.is_none());
    assert!(mark.owed_in(SECOND));
    assert!(!mark.owed_in(FIRST));
}

#[test]
fn idle_turn_in_the_same_start_clears_the_mark() {
    let mut mark = working("session-a", FIRST);

    assert!(observe_turn(&mut mark, None, FIRST));
    assert!(mark.is_none());
    assert!(!observe_turn(&mut mark, None, FIRST));
}

#[test]
fn repeated_working_observation_changes_nothing() {
    let mut mark = working("session-a", FIRST);

    assert!(!observe_turn(&mut mark, Some("session-a"), FIRST));
}

#[test]
fn rotated_session_while_working_follows_the_live_session() {
    let mut mark = working("session-a", FIRST);

    assert!(observe_turn(&mut mark, Some("session-b"), FIRST));
    assert_eq!(mark.unwrap().session_id, "session-b");
}

#[test]
fn owed_mark_is_frozen_until_settled() {
    let mut mark = working("session-a", FIRST);

    assert!(!observe_turn(&mut mark, None, SECOND));
    assert!(!observe_turn(&mut mark, Some("session-x"), SECOND));
    assert_eq!(mark.as_ref().unwrap().session_id, "session-a");
    assert!(mark.as_ref().unwrap().owed_in(SECOND));
}

#[test]
fn owed_mark_settles_exactly_once_per_start() {
    let mut mark = working("session-a", FIRST);

    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));
    assert!(!settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));
    assert!(!settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Skipped("late".into())
    ));
    assert_eq!(
        mark.unwrap().settled,
        Some(TurnResumeSettlement {
            start_epoch: SECOND.into(),
            outcome: TurnResumeOutcome::Sent,
        })
    );
}

#[test]
fn settled_mark_owes_nothing_to_the_next_start() {
    let mut mark = working("session-a", FIRST);
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));

    assert!(!mark.as_ref().unwrap().owed_in(THIRD));
    assert!(!settle_turn_resume(
        &mut mark,
        THIRD,
        TurnResumeOutcome::Sent
    ));
}

#[test]
fn resumed_work_after_the_continue_owes_the_following_start() {
    let mut mark = working("session-a", FIRST);
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));

    assert!(observe_turn(&mut mark, Some("session-a"), SECOND));
    let current = mark.as_ref().unwrap();
    assert_eq!(current.start_epoch, SECOND);
    assert!(current.settled.is_none());
    assert!(current.owed_in(THIRD));
}

#[test]
fn idle_tab_never_owes_a_continue() {
    let mut mark = None;

    assert!(!observe_turn(&mut mark, None, FIRST));
    assert!(mark.is_none());
    assert!(!settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));
}

#[test]
fn request_survives_the_callers_turn_ending() {
    let mut mark = working("session-a", FIRST);
    request_continue(&mut mark, "session-a", FIRST, "opaque-request".into());

    assert!(!observe_turn(&mut mark, None, FIRST));
    assert!(!observe_turn(&mut mark, Some("session-a"), FIRST));
    let current = mark.as_ref().unwrap();
    assert_eq!(current.prompt.as_deref(), Some("opaque-request"));
    assert!(current.owed_in(SECOND));
}

#[test]
fn request_on_an_idle_tab_is_owed_by_the_next_start() {
    let mut mark = None;
    request_continue(&mut mark, "session-a", FIRST, "opaque-request".into());

    assert!(mark.as_ref().unwrap().owed_in(SECOND));
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));
}

#[test]
fn no_continue_suppresses_the_callers_mark_for_this_start() {
    let mut mark = working("session-a", FIRST);

    assert!(suppress_continue(&mut mark, "session-a", FIRST));
    assert!(!observe_turn(&mut mark, Some("session-a"), FIRST));
    assert!(!mark.as_ref().unwrap().owed_in(SECOND));
    assert_eq!(
        mark.as_ref().unwrap().settled.as_ref().unwrap().outcome,
        TurnResumeOutcome::Skipped("not_requested".into())
    );
}

#[test]
fn no_continue_replaces_a_prior_settlement_for_a_current_working_turn() {
    let mut mark = working("session-a", FIRST);
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));

    assert!(suppress_continue(&mut mark, "session-a", SECOND));
    assert!(!observe_turn(&mut mark, Some("session-a"), SECOND));
    let suppressed = mark.as_ref().unwrap();
    assert_eq!(
        suppressed.settled.as_ref().unwrap().outcome,
        TurnResumeOutcome::Skipped("not_requested".into())
    );
    assert!(!suppressed.owed_in(THIRD));
}

#[test]
fn withdrawn_claim_records_the_reason_without_reopening_the_mark() {
    let mut mark = working("session-a", FIRST);
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Sent
    ));

    withdraw_sent_claim(&mut mark, SECOND, "opaque-reason".into());

    let settled = mark.as_ref().unwrap().settled.clone().unwrap();
    assert_eq!(
        settled.outcome,
        TurnResumeOutcome::Skipped("opaque-reason".into())
    );
    assert!(!mark.unwrap().owed_in(THIRD));
}

#[test]
fn mark_round_trips_through_its_persisted_form() {
    let mut mark = working("session-a", FIRST);
    assert!(settle_turn_resume(
        &mut mark,
        SECOND,
        TurnResumeOutcome::Skipped("opaque-reason".into())
    ));

    let json = serde_json::to_string(&mark).unwrap();
    let back: Option<TurnResumeMark> = serde_json::from_str(&json).unwrap();

    assert_eq!(back, mark);
}
