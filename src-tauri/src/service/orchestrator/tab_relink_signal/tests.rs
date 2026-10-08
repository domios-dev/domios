use std::cell::RefCell;
use std::future::{poll_fn, Future};
use std::task::Poll;
use std::time::Duration;

use super::{await_relink, notify_tab_relinked};

#[tokio::test(start_paused = true)]
async fn backend_attachment_stays_pending_past_old_frontend_deadline() {
    let backend = RefCell::new(None::<String>);
    let mut attachment = Box::pin(await_relink(|| backend.borrow().clone()));
    assert!(poll_fn(|cx| Poll::Ready(attachment.as_mut().poll(cx)))
        .await
        .is_pending());
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(poll_fn(|cx| Poll::Ready(attachment.as_mut().poll(cx)))
        .await
        .is_pending());
    backend.replace(Some("ga-owned-pty".into()));
    notify_tab_relinked();
    assert_eq!(attachment.await, "ga-owned-pty");
}

#[tokio::test]
async fn relink_between_subscription_and_predicate_is_not_lost() {
    let mut checks = 0;
    let value = await_relink(|| {
        checks += 1;
        if checks == 1 {
            notify_tab_relinked();
            None
        } else {
            Some("backend-pty")
        }
    })
    .await;
    assert_eq!(value, "backend-pty");
}
