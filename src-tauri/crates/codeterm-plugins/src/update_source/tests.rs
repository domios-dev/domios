use super::*;
#[test]
fn discovery_refuses_untrusted_schemes_and_credentials() {
    assert!(!discovery_url_allowed("file:///tmp/latest.json"));
    assert!(!discovery_url_allowed("http://example.com/latest.json"));
    assert!(!discovery_url_allowed(
        "https://user:secret@example.com/latest.json"
    ));
    assert!(discovery_url_allowed("https://example.com/latest.json"));
}
#[cfg(debug_assertions)]
#[test]
fn fixture_source_fetches_manifests_without_network_and_enforces_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("latest.json"), b"fixture").unwrap();
    let fixture = FixtureUpdateSource {
        root: dir.path().into(),
    };
    let source: &dyn UpdateDiscoverySource = &fixture;
    assert_eq!(
        source
            .fetch("https://example.invalid/latest.json?cache=1", 7)
            .unwrap(),
        b"fixture"
    );
    assert!(source
        .fetch("https://example.invalid/latest.json", 6)
        .is_err());
    assert!(source
        .fetch("https://example.invalid/missing.json", 16)
        .is_err());
}
#[cfg(not(debug_assertions))]
#[test]
fn release_never_selects_fixtures_or_loopback() {
    assert_eq!(fixture_root(), None);
    assert!(!discovery_url_allowed("http://127.0.0.1/latest.json"));
}

#[test]
fn exhausted_github_quota_reports_local_reset_time() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
    headers.insert("x-ratelimit-reset", "1735732800".parse().unwrap());
    let expected = chrono::DateTime::from_timestamp(1735732800, 0)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    for status in [403, 429] {
        assert_eq!(
            rate_limit_error(status, &headers),
            Some(format!("GitHub rate limit, retry at {expected}"))
        );
    }
    assert_eq!(rate_limit_error(404, &headers), None);
    headers.insert("x-ratelimit-remaining", "1".parse().unwrap());
    assert_eq!(rate_limit_error(403, &headers), None);
    headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
    headers.insert("x-ratelimit-reset", "invalid".parse().unwrap());
    assert_eq!(rate_limit_error(429, &headers), None);
}
