use super::*;
use serde_json::json;

fn failure(state: &mut RetryState, turn: &str, signal: serde_json::Value, now: u64) -> Decision {
    state.error = Some((turn.into(), serde_json::from_value(signal).unwrap()));
    state.finish(turn, Some(&RuntimeTurnDetail::Failed), now)
}

fn transient() -> serde_json::Value {
    json!({"retryableKind":"transient","retryable":false})
}

#[test]
fn capacity_delay_uses_injected_clock_and_consumes_error_once() {
    let mut state = RetryState::default();
    assert_eq!(
        failure(&mut state, "turn", transient(), 123),
        Decision::Schedule {
            attempt: 1,
            at: 10_123
        }
    );
    assert_eq!(
        state.finish("turn", Some(&RuntimeTurnDetail::Failed), 123),
        Decision::Ignore
    );
}

#[test]
fn rate_limit_uses_reset_time_on_every_attempt() {
    let mut state = RetryState::default();
    for attempt in 1..=2 {
        assert_eq!(
            failure(
                &mut state,
                "turn",
                json!({"retryableKind":"rate_limited_until","retryAtMs":900000,"retryable":false}),
                123
            ),
            Decision::Schedule {
                attempt,
                at: 900_000
            }
        );
    }
}

#[test]
fn repeat_failures_increment_and_success_resets() {
    let mut state = RetryState::default();
    failure(&mut state, "first", transient(), 1);
    assert_eq!(
        failure(&mut state, "second", transient(), 2),
        Decision::Schedule {
            attempt: 2,
            at: 10_002
        }
    );
    assert_eq!(state.finish("success", None, 3), Decision::Reset);
    assert_eq!(
        failure(&mut state, "next", transient(), 4),
        Decision::Schedule {
            attempt: 1,
            at: 10_004
        }
    );
}

#[test]
fn tenth_failure_stops_once_for_both_error_kinds() {
    for signal in [
        transient(),
        json!({"retryableKind":"rate_limited_until","retryAtMs":900000,"retryable":false}),
    ] {
        let mut state = RetryState::default();
        for attempt in 1..10 {
            assert!(matches!(failure(&mut state, "turn", signal.clone(), 1),
                Decision::Schedule { attempt: actual, .. } if actual == attempt));
        }
        assert_eq!(
            failure(&mut state, "turn", signal.clone(), 1),
            Decision::Stopped
        );
        assert_eq!(failure(&mut state, "turn", signal, 1), Decision::Ignore);
    }
}

#[test]
fn interrupt_does_not_count_as_success_or_retry() {
    let mut state = RetryState::default();
    failure(&mut state, "first", transient(), 1);
    assert_eq!(
        state.finish("interrupted", Some(&RuntimeTurnDetail::Interrupted), 2),
        Decision::Ignore
    );
    assert_eq!(state.attempts, 1);
}
