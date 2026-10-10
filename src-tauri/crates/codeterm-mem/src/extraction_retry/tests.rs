use super::*;

#[test]
fn extraction_retry_cap_backoff_and_parking_survive_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retries.json");
    let retries = ExtractionRetries::load(path.clone());
    assert_eq!(
        retries
            .claim("extract:a", "body1", "w1", 0)
            .unwrap()
            .unwrap()
            .attempt,
        1
    );
    assert!(retries
        .claim("extract:a", "body1", "duplicate", 0)
        .unwrap()
        .is_none());
    let first = retries
        .finish(
            "extract:a",
            "w1",
            false,
            Some("launch failure".into()),
            1000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(first.retry_after, Some(61_000));
    assert!(!first.parked);
    assert!(retries
        .claim("extract:a", "body1", "early", 60_999)
        .unwrap()
        .is_none());
    assert_eq!(
        retries
            .claim("extract:a", "body1", "w2", 61_000)
            .unwrap()
            .unwrap()
            .attempt,
        2
    );
    assert_eq!(
        retries
            .finish(
                "extract:a",
                "w2",
                false,
                Some("launch failure".into()),
                62_000
            )
            .unwrap()
            .unwrap()
            .retry_after,
        Some(182_000)
    );
    assert_eq!(
        retries
            .claim("extract:a", "body1", "w3", 182_000)
            .unwrap()
            .unwrap()
            .attempt,
        3
    );
    let last = retries
        .finish(
            "extract:a",
            "w3",
            false,
            Some("launch failure".into()),
            183_000,
        )
        .unwrap()
        .unwrap();
    assert!(last.parked);
    assert_eq!(last.retry_after, None);
    assert_eq!(last.reason.as_deref(), Some("launch failure"));
    let reloaded = ExtractionRetries::load(path);
    assert!(reloaded
        .claim("extract:a", "body1", "w4", u64::MAX)
        .unwrap()
        .is_none());
    reloaded.reset("extract:a").unwrap();
    assert_eq!(
        reloaded
            .claim("extract:a", "body1", "manual", 0)
            .unwrap()
            .unwrap()
            .attempt,
        1
    );
}

#[test]
fn extraction_retry_cause_change_and_success_without_graph_children() {
    let dir = tempfile::tempdir().unwrap();
    let retries = ExtractionRetries::load(dir.path().join("retries.json"));
    retries.claim("extract:a", "unavailable", "w1", 0).unwrap();
    retries
        .finish("extract:a", "w1", false, None, 1000)
        .unwrap();
    assert_eq!(
        retries
            .claim("extract:a", "available", "w2", 1001)
            .unwrap()
            .unwrap()
            .attempt,
        1
    );
    assert!(retries
        .finish("extract:a", "w1", false, None, 2000)
        .unwrap()
        .is_none());
    retries.finish("extract:a", "w2", true, None, 2000).unwrap();
    assert!(retries
        .claim("extract:a", "available", "w3", u64::MAX)
        .unwrap()
        .is_none());
    assert!(retries
        .claim("extract:a", "changed-body", "w4", 2001)
        .unwrap()
        .is_some());
    assert_ne!(fingerprint(&["ab", "c"]), fingerprint(&["a", "bc"]));
}

#[test]
fn extraction_retry_corrupt_store_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retries.json");
    std::fs::write(&path, b"broken").unwrap();
    let retries = ExtractionRetries::load(path);
    assert!(retries.claim("extract:a", "body1", "w1", 0).is_err());
}

#[test]
fn extraction_retry_recovers_crash_between_claim_and_registration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retries.json");
    let retries = ExtractionRetries::load(path.clone());
    retries.claim("extract:a", "body1", "w1", 0).unwrap();
    assert!(retries.reset("extract:a").is_err());
    assert!(retries
        .claim("extract:a", "changed", "duplicate", 1)
        .unwrap()
        .is_none());
    let reloaded = ExtractionRetries::load(path);
    reloaded.abandon_unfinished(1000).unwrap();
    assert_eq!(reloaded.failed_intents().unwrap(), vec!["extract:a"]);
    assert!(reloaded
        .claim("extract:a", "body1", "early", 60_999)
        .unwrap()
        .is_none());
    assert_eq!(
        reloaded
            .claim("extract:a", "body1", "w2", 61_000)
            .unwrap()
            .unwrap()
            .attempt,
        2
    );
}
