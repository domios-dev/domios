use super::*;
use std::future::Future;
use std::task::{Context, Poll};

#[test]
fn same_tab_waits_for_the_active_transition() {
    let first = acquire("activation-serial").unwrap();
    let mut second = Box::pin(acquire_async("activation-serial".into()));
    let mut context = Context::from_waker(futures_util::task::noop_waker_ref());
    assert!(matches!(second.as_mut().poll(&mut context), Poll::Pending));
    drop(first);
    assert!(matches!(
        second.as_mut().poll(&mut context),
        Poll::Ready(Ok(_))
    ));
}

#[test]
fn different_tabs_do_not_block_each_other() {
    let _first = acquire("activation-a").unwrap();
    let _second = acquire("activation-b").unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn held_activation_bounds_contenders_and_cancellation_frees_admission_without_blocking_runtime(
) {
    let first = acquire_async("activation-bounded".into()).await.unwrap();
    let mut context = Context::from_waker(futures_util::task::noop_waker_ref());
    let mut waiters = Vec::new();
    for _ in 1..admission::MAX_CONTENDERS {
        let mut waiter = Box::pin(acquire_async("activation-bounded".into()));
        assert!(matches!(waiter.as_mut().poll(&mut context), Poll::Pending));
        waiters.push(waiter);
    }
    let error = acquire_async("activation-bounded".into())
        .await
        .err()
        .unwrap();
    assert!(error.contains("contender capacity"));
    let independent_task = tokio::spawn(async { "progress" });
    let independent = acquire_async("activation-independent".into())
        .await
        .unwrap();
    assert_eq!(independent_task.await.unwrap(), "progress");
    drop(independent);
    waiters.clear();
    let mut replacement = Box::pin(acquire_async("activation-bounded".into()));
    assert!(matches!(
        replacement.as_mut().poll(&mut context),
        Poll::Pending
    ));
    drop(first);
    assert!(matches!(
        replacement.as_mut().poll(&mut context),
        Poll::Ready(Ok(_))
    ));
}
