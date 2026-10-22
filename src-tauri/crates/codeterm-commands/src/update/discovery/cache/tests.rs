use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn stale_concurrent_checks_share_success_and_failure() {
    for result in [Ok(42), Err("offline".into())] {
        let flight = SingleFlight::new();
        let calls = AtomicUsize::new(0);
        let (entered, entered_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        let first = flight.run(0, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            entered.send(()).unwrap();
            release_rx.await.unwrap();
            result.clone()
        });
        let second = async {
            entered_rx.await.unwrap();
            let pending = flight.run(0, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(99)
            });
            tokio::pin!(pending);
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(
                    pending.as_mut().poll(cx).is_pending()
                ))
                .await
            );
            release.send(()).unwrap();
            pending.await
        };
        let (one, two) = tokio::join!(first, second);
        assert_eq!(one, result);
        assert_eq!(two, result);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn cache_age_bounds_success_and_failure_attempts() {
    assert!(!fresh(None, 100_000));
    assert!(fresh(Some(70_001), 100_000));
    assert!(!fresh(Some(70_000), 100_000));
    assert!(!fresh(Some(100_001), 100_000));
}
