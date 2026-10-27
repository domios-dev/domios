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
