use super::*;

fn key(peer: &str) -> BackoffKey {
    (1, peer.to_owned())
}

#[test]
fn exponential_backoff_grows_with_jitter_and_caps() {
    let mut backoff = PeerStreamBackoff::new(1_000, 30_000, 10_000, 16);
    let mut now = 10_000;
    let expected_ceilings = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000];
    for (idx, ceiling) in expected_ceilings.iter().enumerate() {
        let jitter = if idx % 2 == 0 { 0.0 } else { 1.0 };
        let outcome = backoff.record_refusal(key("ubunta"), Some("nonce-1"), now, jitter, None);
        let expected_delay = if idx % 2 == 0 { *ceiling / 2 } else { *ceiling };
        assert_eq!(outcome.delay_ms, expected_delay);
        assert_eq!(outcome.attempts, (idx + 1) as u32);
        assert_eq!(
            backoff.check_allowed(&key("ubunta"), Some("nonce-1"), now + expected_delay - 1),
            Err(1)
        );
        assert_eq!(
            backoff.check_allowed(&key("ubunta"), Some("nonce-1"), now + expected_delay),
            Ok(())
        );
        now += expected_delay;
    }
}

#[test]
fn refusal_triggers_rate_limited_refresh_and_resets_on_relink_or_success() {
    let mut backoff = PeerStreamBackoff::new(1_000, 30_000, 10_000, 16);
    let first = backoff.record_refusal(key("ubunta"), Some("nonce-1"), 5_000, 1.0, None);
    assert!(first.trigger_refresh);
    assert_eq!(
        backoff.check_allowed(&key("ubunta"), Some("nonce-1"), 5_500),
        Err(500)
    );

    let second = backoff.record_refusal(key("ubunta"), Some("nonce-1"), 6_000, 1.0, None);
    assert!(!second.trigger_refresh);

    let third = backoff.record_refusal(key("ubunta"), Some("nonce-1"), 15_000, 1.0, None);
    assert!(third.trigger_refresh);

    // Fresh link nonce clears stale refusal backoff immediately.
    assert_eq!(
        backoff.check_allowed(&key("ubunta"), Some("nonce-2"), 15_100),
        Ok(())
    );

    backoff.record_refusal(key("ubunta"), Some("nonce-2"), 20_000, 1.0, None);
    assert!(backoff
        .check_allowed(&key("ubunta"), Some("nonce-2"), 20_500)
        .is_err());
    backoff.record_success(&key("ubunta"));
    assert_eq!(
        backoff.check_allowed(&key("ubunta"), Some("nonce-2"), 20_500),
        Ok(())
    );
}

#[test]
fn honors_retry_after_floor_clamped_to_cap_for_503_and_401() {
    assert!(is_backoff_status(StatusCode::UNAUTHORIZED));
    assert!(is_backoff_status(StatusCode::SERVICE_UNAVAILABLE));
    assert!(!is_backoff_status(StatusCode::FORBIDDEN));
    assert_eq!(parse_retry_after_ms(Some("2")), Some(2_000));
    assert_eq!(parse_retry_after_ms(Some(" 5 ")), Some(5_000));
    assert_eq!(parse_retry_after_ms(Some("0")), None);
    assert_eq!(parse_retry_after_ms(Some("invalid")), None);
    assert_eq!(parse_retry_after_ms(None), None);

    let mut backoff = PeerStreamBackoff::new(1_000, 30_000, 10_000, 16);
    let first = backoff.record_refusal(key("ubunta"), Some("nonce-1"), 1_000, 0.0, Some(5_000));
    assert_eq!(first.delay_ms, 5_000);
    assert!(first.trigger_refresh);
    assert_eq!(
        backoff.check_allowed(&key("ubunta"), Some("nonce-1"), 5_999),
        Err(1)
    );
    assert_eq!(
        backoff.check_allowed(&key("ubunta"), Some("nonce-1"), 6_000),
        Ok(())
    );

    // Oversized Retry-After is clamped to max_delay_ms.
    let clamped = backoff.record_refusal(key("ubunta"), Some("nonce-1"), 6_000, 0.0, Some(120_000));
    assert_eq!(clamped.delay_ms, 30_000);
}

#[test]
fn bounds_tracked_peers() {
    let mut backoff = PeerStreamBackoff::new(1_000, 30_000, 10_000, 2);
    backoff.record_refusal(key("peer-a"), None, 100, 1.0, None);
    backoff.record_refusal(key("peer-b"), None, 200, 1.0, None);
    backoff.record_refusal(key("peer-c"), None, 300, 1.0, None);
    assert_eq!(backoff.len(), 2);
}
