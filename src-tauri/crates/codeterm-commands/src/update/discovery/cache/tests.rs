use super::*;

#[tokio::test]
async fn cached_snapshot_returns_without_waiting_for_fetch() {
    let (send, receive) = tokio::sync::oneshot::channel::<u32>();
    let job = std::sync::Mutex::new(None);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        serve(
            7,
            false,
            || async { Ok(receive.await.unwrap()) },
            |refresh| {
                *job.lock().unwrap() = Some(refresh);
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, 7);
    send.send(9).unwrap();
    assert_eq!(job.into_inner().unwrap().unwrap().await.unwrap(), 9);
}

#[tokio::test]
async fn forced_refresh_returns_the_fetched_snapshot() {
    assert_eq!(
        serve(
            7,
            true,
            || async { Ok(9) },
            |_| panic!("forced refresh queued")
        )
        .await
        .unwrap(),
        9
    );
}
#[test]
fn cache_age_requires_a_recent_completed_check() {
    assert!(!fresh(None, 100_000));
    assert!(fresh(Some(70_001), 100_000));
    assert!(!fresh(Some(70_000), 100_000));
    assert!(!fresh(Some(100_001), 100_000));
}

#[tokio::test]
async fn stale_plain_check_waits_for_new_manifest_and_propagates_failure() {
    for checked_at in [None, Some(1)] {
        let result = serve(
            "old",
            !fresh(checked_at, 100_000),
            || async { Ok("new") },
            |_| panic!("stale check queued"),
        )
        .await
        .unwrap();
        assert_eq!(result, "new");
        assert!(serve(
            "old",
            !fresh(checked_at, 100_000),
            || async { Err("offline".into()) },
            |_| panic!("stale check queued")
        )
        .await
        .is_err());
    }
}
