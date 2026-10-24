use super::*;

#[test]
fn ten_fresh_attempts_use_injected_time_and_then_exhaust() {
    let mut policy = Policy::default();
    policy.begin("original", "opaque turn");
    for attempt in 1..=MAX_ATTEMPTS {
        let turn = format!("turn-{attempt}");
        let send = policy.send_id.clone();
        policy.bind(&send, &turn);
        policy.error(&turn, Failure::Transient);
        assert_eq!(
            policy.finish(&turn, true, 123),
            Decision::Schedule {
                attempt,
                at: 10_123,
                failure: Failure::Transient
            }
        );
        assert_eq!(policy.finish(&turn, true, 123), Decision::Ignore);
        policy.pending = Some("delayed".into());
        assert_eq!(
            policy.claim("delayed", &format!("retry-{attempt}")),
            Some("opaque turn".into())
        );
    }
    let send = policy.send_id.clone();
    policy.bind(&send, "final");
    policy.error("final", Failure::Transient);
    assert_eq!(
        policy.finish("final", true, 123),
        Decision::Exhausted(Failure::Transient)
    );
}

#[test]
fn exact_turn_explicit_capacity_and_terminal_failure_are_all_required() {
    for (send, error_turn, finish_turn, failed) in [
        ("foreign", "own", "own", true),
        ("send", "foreign", "own", true),
        ("send", "own", "foreign", true),
        ("send", "own", "own", false),
    ] {
        let mut policy = Policy::default();
        policy.begin("send", "opaque");
        policy.bind(send, "own");
        policy.error(error_turn, Failure::Transient);
        assert_eq!(policy.finish(finish_turn, failed, 1), Decision::Ignore);
    }
}

#[test]
fn cancel_and_new_send_revoke_pending_and_old_turn_authority() {
    for cancel in [true, false] {
        let mut policy = Policy::default();
        policy.begin("send", "opaque");
        policy.bind("send", "turn");
        policy.error("turn", Failure::Transient);
        policy.pending = Some("delayed".into());
        if cancel {
            assert_eq!(policy.cancel(), Some("delayed".into()));
        } else {
            policy.begin("new-send", "new text");
        }
        assert_eq!(policy.claim("delayed", "retry"), None);
        assert_eq!(policy.finish("turn", true, 0), Decision::Ignore);
    }
}

#[test]
fn reset_time_gets_one_attempt_and_never_a_fast_retry() {
    let mut policy = Policy::default();
    policy.begin("original", "opaque turn");
    policy.bind("original", "first");
    policy.error("first", Failure::RateLimitedUntil(500_000));
    assert_eq!(
        policy.finish("first", true, 123),
        Decision::Schedule {
            attempt: 1,
            at: 500_000,
            failure: Failure::RateLimitedUntil(500_000)
        }
    );
    policy.pending = Some("delayed".into());
    assert!(policy.claim("delayed", "reset-retry").is_some());
    policy.bind("reset-retry", "second");
    policy.error("second", Failure::RateLimitedUntil(900_000));
    assert_eq!(
        policy.finish("second", true, 500_123),
        Decision::Exhausted(Failure::RateLimitedUntil(900_000))
    );
}

#[test]
fn past_or_invalid_reset_times_never_authorize_a_retry() {
    for at in [0, 123, u64::MAX] {
        let mut policy = Policy::default();
        policy.begin("original", "opaque");
        policy.bind("original", "turn");
        policy.error("turn", Failure::RateLimitedUntil(at));
        assert_eq!(policy.finish("turn", true, 123), Decision::Ignore);
    }
}

#[test]
fn cancellation_revokes_claimed_retry_through_later_send_preparation() {
    let mut policy = Policy::default();
    policy.begin("original", "opaque");
    policy.pending = Some("delayed".into());
    policy.claim("delayed", "retry").unwrap();
    assert!(policy.permits_send("retry"));
    policy.cancel();
    assert!(!policy.permits_send("retry"));
    policy.begin("new-user-send", "new text");
    assert!(!policy.permits_send("retry"));
    assert!(policy.permits_send("new-user-send"));
}

#[test]
fn a_later_permanent_error_clears_earlier_transient_authority() {
    let mut policy = Policy::default();
    policy.begin("original", "opaque");
    policy.bind("original", "turn");
    policy.error("turn", Failure::Transient);
    policy.clear_error("turn");
    assert_eq!(policy.finish("turn", true, 123), Decision::Ignore);
}
