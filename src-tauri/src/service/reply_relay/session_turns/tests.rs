use super::*;
use crate::agent::provider::{
    ReplyRelayEndedTurn, ReplyRelayOutcome, ReplyRelayTurn, ReplyRelayUnavailable,
};

fn answered(offset: u64) -> ReplyRelayEvidence {
    ReplyRelayEvidence::Answered(ReplyRelayTurn {
        user_turn_id: format!("user-{offset}"),
        user_record_start: offset,
        user_text: format!("opaque user {offset}\nsecond line"),
        assistant_turn_id: format!("assistant-{offset}"),
        answer: format!("answer {offset}"),
        complete: true,
    })
}

#[test]
fn session_turns_keep_last_requested_completed_answers_and_raw_users() {
    let result = collect_turns(2, 0, |offset| {
        [10, 20, 30]
            .into_iter()
            .find(|candidate| *candidate >= offset)
            .map(answered)
    })
    .unwrap();
    assert_eq!(result.turns.len(), 2);
    assert_eq!(result.turns[0].user_record_start, 20);
    assert_eq!(result.turns[1].assistant_text.as_deref(), Some("answer 30"));
    assert_eq!(result.turns[1].user_text, "opaque user 30\nsecond line");
    assert_eq!(result.next_offset, 31);
    assert!(!result.has_more);
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["turns"][1]["outcome"], "answered");
}

#[test]
fn session_turns_report_non_success_and_unavailable_without_invented_answers() {
    let result = collect_turns(10, 0, |offset| {
        (offset == 0).then(|| {
            ReplyRelayEvidence::Ended(ReplyRelayEndedTurn {
                user_turn_id: "u".into(),
                user_record_start: 5,
                user_text: "opaque".into(),
                complete: false,
                outcome: ReplyRelayOutcome::Cancelled,
            })
        })
    })
    .unwrap();
    assert_eq!(result.turns[0].outcome, "cancelled");
    assert!(result.turns[0].assistant_text.is_none());
    let unavailable = collect_turns(1, 0, |_| {
        Some(ReplyRelayEvidence::Unavailable(ReplyRelayUnavailable {
            unavailable: true,
            reason: "plain mode".into(),
        }))
    })
    .unwrap();
    assert_eq!(
        unavailable.unavailable_reason.as_deref(),
        Some("plain mode")
    );
    assert!(unavailable.turns.is_empty());
}

#[test]
fn session_turns_bound_scan_and_require_progress() {
    let mut calls = 0;
    let result = collect_turns(2, 0, |offset| {
        calls += 1;
        Some(answered(offset))
    })
    .unwrap();
    assert_eq!(calls, MAX_SCAN_TURNS + 1);
    assert!(result.has_more);
    assert_eq!(result.next_offset, MAX_SCAN_TURNS as u64);
    assert!(collect_turns(1, 20, |_| Some(answered(10))).is_err());
    assert!(collect_turns(0, 0, |_| panic!("invalid limit must not read provider")).is_err());
    assert!(collect_turns(51, 0, |_| None).is_err());
}

#[test]
fn session_turns_refuse_incomplete_or_oversized_evidence() {
    let mut invalid = answered(1);
    if let ReplyRelayEvidence::Answered(turn) = &mut invalid {
        turn.complete = false;
    }
    assert!(collect_turns(1, 0, |_| Some(invalid.clone())).is_err());
    let mut huge = answered(1);
    if let ReplyRelayEvidence::Answered(turn) = &mut huge {
        turn.answer = "x".repeat(MAX_TURN_BYTES);
    }
    assert!(collect_turns(1, 0, |_| Some(huge.clone())).is_err());
}

#[test]
fn session_turns_require_current_manager_or_owner_for_agent_callers() {
    assert!(may_read_turns(None, "worker", false, None, None));
    assert!(may_read_turns(
        Some("manager"),
        "worker",
        true,
        Some("manager"),
        None
    ));
    assert!(may_read_turns(
        Some("orch"),
        "worker",
        true,
        None,
        Some("orch")
    ));
    assert!(may_read_turns(Some("worker"), "worker", true, None, None));
    assert!(!may_read_turns(
        Some("manager"),
        "worker",
        false,
        Some("manager"),
        None
    ));
    assert!(!may_read_turns(
        Some("foreign"),
        "worker",
        true,
        Some("manager"),
        Some("orch")
    ));
}
