use super::*;

#[test]
fn rate_limit_waits_until_the_injected_deadline_and_success_resets() {
    let mut retry = Retry::default();
    let refusal = AccountMeshRefusal::RateLimited {
        retry_after_seconds: 7,
    };
    retry.settle(100, Err(refusal));
    assert_eq!(retry.deadline, Some(130));
    assert_eq!(retry.pending(129), Some(refusal));
    assert_eq!(retry.pending(130), None);
    retry.settle(130, Ok(()));
    assert_eq!(retry.deadline, None);
    assert_eq!(retry.pending(131), None);
}

#[test]
fn outages_prevent_bursts_and_back_off_to_a_cap() {
    let mut retry = Retry::default();
    let mut now = 100;
    for delay in [30, 60, 120, 240] {
        retry.settle(now, Err(AccountMeshRefusal::Unavailable));
        assert_eq!(retry.deadline, Some(now + delay));
        assert_eq!(
            retry.pending(now + delay - 1),
            Some(AccountMeshRefusal::Unavailable)
        );
        now += delay;
        assert_eq!(retry.pending(now), None);
    }
    retry.settle(now, Err(AccountMeshRefusal::Unavailable));
    assert!(retry.exhausted());
    assert_eq!(retry.deadline, None);
    assert_eq!(
        retry.pending(now + 86_400),
        Some(AccountMeshRefusal::Unavailable)
    );
    retry.resume();
    assert_eq!(retry.pending(now + 86_400), None);
    assert!(!retry.exhausted());
    retry.settle(now, Ok(()));
    retry.settle(now, Err(AccountMeshRefusal::Unavailable));
    assert_eq!(retry.deadline, Some(now + 30));
}

#[test]
fn rate_limit_uses_the_longer_server_or_exponential_backoff_deadline() {
    let mut retry = Retry::default();
    retry.settle(100, Err(AccountMeshRefusal::Unavailable));
    retry.settle(
        130,
        Err(AccountMeshRefusal::RateLimited {
            retry_after_seconds: 7,
        }),
    );
    assert_eq!(retry.deadline, Some(190));
    retry.settle(
        190,
        Err(AccountMeshRefusal::RateLimited {
            retry_after_seconds: 180,
        }),
    );
    assert_eq!(retry.deadline, Some(370));
}

#[test]
fn enrollment_budget_arms_one_retry_and_parks_after_a_second_refusal() {
    for seconds in [None, Some(61)] {
        let mut retry = Retry::default();
        let refusal = AccountMeshRefusal::EnrollmentBudget {
            retry_after_seconds: seconds,
        };
        let delay = seconds.unwrap_or(900);
        retry.settle(100, Err(refusal));
        assert_eq!(retry.deadline, Some(100 + delay));
        assert_eq!(retry.pending(99 + delay), Some(refusal));
        assert_eq!(retry.pending(100 + delay), None);
        retry.settle(100 + delay, Err(refusal));
        assert_eq!(retry.deadline, None);
        retry.resume();
        assert_eq!(retry.pending(100 + delay + 86400), Some(refusal));
        retry.settle(100 + delay, Ok(()));
        assert_eq!(retry.pending(100 + delay), None);
    }
}

#[test]
fn permanent_refusals_back_off_to_hours_and_success_clears_the_history() {
    let mut retry = Retry::default();
    let mut now = 100;
    for delay in [300, 600, 1200, 2400, 4800, 9600, 14400, 14400] {
        retry.settle(now, Err(AccountMeshRefusal::Forbidden));
        assert_eq!(retry.deadline, Some(now + delay));
        assert_eq!(
            retry.pending(now + delay - 1),
            Some(AccountMeshRefusal::Forbidden)
        );
        now += delay;
        assert_eq!(retry.pending(now), None);
        assert!(!retry.exhausted());
    }
    retry.settle(now, Ok(()));
    retry.settle(now, Err(AccountMeshRefusal::Invalid));
    assert_eq!(retry.deadline, Some(now + 300));
}

#[test]
fn permanent_backoff_resume_rearms_without_waiting_for_its_deadline() {
    let mut retry = Retry::default();
    retry.settle(100, Err(AccountMeshRefusal::Forbidden));
    assert_eq!(retry.pending(101), Some(AccountMeshRefusal::Forbidden));
    retry.resume();
    assert_eq!(retry.pending(101), None);
    assert_eq!(retry.deadline, None);
    retry.settle(101, Err(AccountMeshRefusal::Forbidden));
    assert_eq!(retry.deadline, Some(401));
}

#[test]
fn budget_after_other_failures_keeps_its_single_deadline_through_resume() {
    for previous in [
        AccountMeshRefusal::Forbidden,
        AccountMeshRefusal::Unavailable,
    ] {
        let mut retry = Retry::default();
        for now in 0..6 {
            retry.settle(now, Err(previous));
        }
        let budget = AccountMeshRefusal::EnrollmentBudget {
            retry_after_seconds: Some(61),
        };
        retry.settle(100, Err(budget));
        assert_eq!(retry.deadline, Some(161));
        assert!(!retry.exhausted());
        retry.resume();
        assert_eq!(retry.deadline, Some(161));
        assert_eq!(retry.pending(160), Some(budget));
        retry.settle(161, Err(budget));
        retry.keep_recovering(161);
        retry.resume();
        assert_eq!(retry.deadline, None);
        assert_eq!(retry.pending(162), Some(budget));
    }
}

#[test]
fn manual_escape_from_other_backoffs_preserves_the_consumed_budget_retry() {
    for refusal in [
        AccountMeshRefusal::Forbidden,
        AccountMeshRefusal::Unavailable,
    ] {
        let mut retry = Retry::default();
        let budget = AccountMeshRefusal::EnrollmentBudget {
            retry_after_seconds: None,
        };
        retry.settle(100, Err(budget));
        for now in 1000..1006 {
            retry.settle(now, Err(refusal));
        }
        retry.resume();
        assert_eq!(retry.pending(1006), None);
        assert_eq!(retry.deadline, None);
        retry.settle(1006, Err(budget));
        assert_eq!(retry.deadline, None);
        assert_eq!(retry.pending(1007), Some(budget));
    }
}
