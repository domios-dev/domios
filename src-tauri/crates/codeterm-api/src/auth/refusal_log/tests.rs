use super::*;

fn key(ip: &str, path: &str) -> RefusalLogKey {
    RefusalLogKey {
        remote_ip: ip.to_owned(),
        path: path.to_owned(),
        credential_kind: "mesh-signature",
        reason: "unknown_or_invalid_credential",
    }
}

#[test]
fn coalesces_repeated_refusals_within_window_and_reports_suppressed_count() {
    let mut coalescer = RefusalLogCoalescer::new(10_000, 16);
    assert_eq!(
        coalescer.observe(key("192.0.2.45", "/api/events/stream"), 1_000),
        RefusalLogDecision::Emit { suppressed: 0 }
    );
    for tick in 1..=441 {
        assert_eq!(
            coalescer.observe(key("192.0.2.45", "/api/events/stream"), 1_000 + tick),
            RefusalLogDecision::Suppress
        );
    }
    assert_eq!(
        coalescer.observe(key("192.0.2.45", "/api/events/stream"), 11_000),
        RefusalLogDecision::Emit { suppressed: 441 }
    );
    assert_eq!(
        coalescer.observe(key("192.0.2.45", "/api/events/stream"), 11_500),
        RefusalLogDecision::Suppress
    );
    assert_eq!(
        coalescer.observe(key("192.0.2.45", "/api/events/stream"), 21_000),
        RefusalLogDecision::Emit { suppressed: 1 }
    );
}

#[test]
fn bounds_tracked_keys_by_evicting_expired_then_oldest() {
    let mut coalescer = RefusalLogCoalescer::new(10_000, 2);
    assert_eq!(
        coalescer.observe(key("198.51.100.1", "/api/events/stream"), 100),
        RefusalLogDecision::Emit { suppressed: 0 }
    );
    assert_eq!(
        coalescer.observe(key("198.51.100.2", "/api/events/stream"), 200),
        RefusalLogDecision::Emit { suppressed: 0 }
    );
    assert_eq!(
        coalescer.observe(key("198.51.100.3", "/api/events/stream"), 300),
        RefusalLogDecision::Emit { suppressed: 0 }
    );
    assert_eq!(coalescer.len(), 2);
    assert_eq!(
        coalescer.observe(key("198.51.100.1", "/api/events/stream"), 400),
        RefusalLogDecision::Emit { suppressed: 0 }
    );
}
