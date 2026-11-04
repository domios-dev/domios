use super::*;

fn reply(status: u16, version: &str, etag: Option<&str>, retry: Option<&str>) -> Reply {
    Reply {
        status,
        etag: etag.map(str::to_owned),
        retry_after: retry.map(str::to_owned),
        body: format!("{{\"version\":\"{version}\"}}").into_bytes(),
    }
}

#[tokio::test(start_paused = true)]
async fn conditional_discovery_retains_304_content_and_replaces_a_changed_offer() {
    let mut endpoint = Endpoint::default();
    let clock = tokio::time::Instant::now;
    let first = endpoint
        .check(clock, SystemTime::UNIX_EPOCH, |etag| async move {
            assert_eq!(etag, None);
            Ok(reply(200, "1.0.0", Some("\"first\""), None))
        })
        .await
        .unwrap();
    assert_eq!(first.version, "1.0.0");
    assert_eq!(
        endpoint
            .check(clock, SystemTime::UNIX_EPOCH, |_| async {
                panic!("throttled")
            })
            .await
            .unwrap(),
        first
    );
    tokio::time::advance(EVENT_THROTTLE).await;
    assert_eq!(
        endpoint
            .check(clock, SystemTime::UNIX_EPOCH, |etag| async move {
                assert_eq!(etag.as_deref(), Some("\"first\""));
                Ok(reply(304, "", None, None))
            })
            .await
            .unwrap(),
        first
    );
    tokio::time::advance(crate::discovery_policy::INTERVAL).await;
    let newer = endpoint
        .check(clock, SystemTime::UNIX_EPOCH, |etag| async move {
            assert_eq!(etag.as_deref(), Some("\"first\""));
            Ok(reply(200, "2.0.0", Some("\"second\""), None))
        })
        .await
        .unwrap();
    assert_eq!(newer.version, "2.0.0");
    assert_eq!(endpoint.etag.as_deref(), Some("\"second\""));
    assert_eq!(endpoint.failures, 0);
}

#[tokio::test(start_paused = true)]
async fn limited_endpoints_preserve_diagnostics_and_never_retry_before_the_deadline() {
    for status in [403, 429] {
        let mut endpoint = Endpoint::default();
        let clock = tokio::time::Instant::now;
        let error = endpoint
            .check(clock, SystemTime::UNIX_EPOCH, |_| async {
                Ok(reply(status, "", None, Some("120")))
            })
            .await
            .unwrap_err();
        assert!(error.contains(&status.to_string()));
        tokio::time::advance(Duration::from_secs(119)).await;
        assert_eq!(
            endpoint
                .check(clock, SystemTime::UNIX_EPOCH, |_| async {
                    panic!("too early")
                })
                .await
                .unwrap_err(),
            error
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(endpoint
            .check(clock, SystemTime::UNIX_EPOCH, |_| async {
                Ok(reply(200, "2.0.0", None, None))
            })
            .await
            .is_ok());
    }
}

#[tokio::test(start_paused = true)]
async fn offline_failures_back_off_and_success_resets_the_budget() {
    let mut endpoint = Endpoint::default();
    for attempt in 1..=10 {
        let start = tokio::time::Instant::now();
        assert!(endpoint
            .check(
                tokio::time::Instant::now,
                SystemTime::UNIX_EPOCH,
                |_| async { Err("offline".into()) }
            )
            .await
            .is_err());
        assert_eq!(endpoint.next, Some(start + backoff(attempt)));
        assert!(backoff(attempt) <= crate::discovery_policy::INTERVAL);
        tokio::time::advance(backoff(attempt)).await;
    }
    endpoint
        .check(
            tokio::time::Instant::now,
            SystemTime::UNIX_EPOCH,
            |_| async { Ok(reply(200, "1.0.0", None, None)) },
        )
        .await
        .unwrap();
    assert_eq!(endpoint.failures, 0);
    assert_eq!(endpoint.error, None);
}

#[test]
fn retry_after_supports_seconds_dates_and_minimum_spacing() {
    let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(784111777);
    assert_eq!(
        retry_after(Some("120"), wall),
        Some(Duration::from_secs(120))
    );
    assert_eq!(
        retry_after(Some("Sun, 06 Nov 1994 08:51:37 GMT"), wall),
        Some(Duration::from_secs(120))
    );
    assert_eq!(retry_after(Some("0"), wall), Some(EVENT_THROTTLE));
    assert_eq!(retry_after(Some("broken"), wall), None);
    assert_eq!(retry_after(None, wall), None);
}

#[test]
fn redirect_targets_require_safe_transport_and_no_embedded_credentials() {
    for url in [
        "https://release-assets.githubusercontent.com/file",
        "https://github.com/repo/releases/latest/download/domios-latest.json",
    ] {
        assert!(allowed(&reqwest::Url::parse(url).unwrap()));
    }
    for url in [
        "https://user:secret@example.com/feed",
        "http://example.com/feed",
        "file:///feed",
    ] {
        assert!(!allowed(&reqwest::Url::parse(url).unwrap()));
    }
}
