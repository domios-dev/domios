use super::*;
use std::future::{poll_fn, Future};
use std::task::Poll;

#[tokio::test]
async fn missing_first_document_acknowledgement_becomes_failure_never_ready() {
    let initial = InitialDocument::default();
    initial.begin(1);
    let (expired, deadline) = tokio::sync::oneshot::channel();
    let mut wait = Box::pin(initial.observe_failure(1, async {
        deadline.await.unwrap();
        Err(())
    }));
    assert!(poll_fn(|cx| Poll::Ready(wait.as_mut().poll(cx)))
        .await
        .is_pending());
    assert_eq!(*initial.0.borrow(), Phase::Awaiting(1));
    expired.send(()).unwrap();
    assert!(wait.await);
    assert!(initial.failed(1));
    assert!(!initial.acknowledge(1));
    assert_eq!(*initial.0.borrow(), Phase::Failed(1));
}

#[tokio::test]
async fn late_acknowledgement_or_failure_cannot_replace_a_newer_surface() {
    let initial = InitialDocument::default();
    initial.begin(1);
    assert!(initial.transition(1, Phase::Failed(1)));
    initial.begin(2);
    assert!(!initial.acknowledge(1));
    assert!(
        !initial
            .observe_failure(1, std::future::ready(Err(())))
            .await
    );
    assert_eq!(*initial.0.borrow(), Phase::Awaiting(2));
    assert!(initial.acknowledge(2));
    assert!(
        !initial
            .observe_failure(2, std::future::ready(Err(())))
            .await
    );
    initial.retire();
    assert!(!initial.acknowledge(2));
    assert_eq!(*initial.0.borrow(), Phase::Absent);
}
