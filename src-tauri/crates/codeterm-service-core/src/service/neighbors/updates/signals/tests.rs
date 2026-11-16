use super::*;

struct Held {
    ready: Option<tokio::sync::oneshot::Sender<()>>,
    closed: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.closed.take().unwrap().send(());
    }
}
#[async_trait::async_trait]
impl RegistryListener for Held {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        self.ready.take().unwrap().send(()).unwrap();
        std::future::pending().await
    }
}

#[tokio::test]
async fn local_lock_drops_signal_stream_even_while_its_consumer_is_busy() {
    let (change, changes) = watch::channel(0);
    let (ready, started) = tokio::sync::oneshot::channel();
    let (closed, ended) = tokio::sync::oneshot::channel();
    let mut listener = guard(
        Box::new(Held {
            ready: Some(ready),
            closed: Some(closed),
        }),
        changes,
    );
    started.await.unwrap();
    change.send_modify(|revision| *revision += 1);
    ended.await.unwrap();
    assert!(matches!(listener.next().await, Ok(Event::PaidChanged)));
    drop(listener);
}

#[tokio::test]
async fn paid_change_after_parent_poll_is_distinct_from_registry_eof() {
    use std::{future::Future, task::Poll};

    let (change, mut changes) = watch::channel(0);
    let (ready, started) = tokio::sync::oneshot::channel();
    let (closed, ended) = tokio::sync::oneshot::channel();
    let mut listener = guard(
        Box::new(Held {
            ready: Some(ready),
            closed: Some(closed),
        }),
        changes.clone(),
    );
    started.await.unwrap();
    let mut parent_change = Box::pin(changes.changed());
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(parent_change.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    change.send_modify(|revision| *revision += 1);
    ended.await.unwrap();
    assert!(matches!(listener.next().await, Ok(Event::PaidChanged)));
    parent_change.await.unwrap();
}

struct Ended(Option<codeterm_neighbors::Error>);

#[async_trait::async_trait]
impl RegistryListener for Ended {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        match self.0.take() {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
}

#[tokio::test]
async fn registry_eof_and_errors_remain_transport_failures() {
    let (_change, changes) = watch::channel(0);
    let mut eof = guard(Box::new(Ended(None)), changes.clone());
    assert!(matches!(eof.next().await, Ok(Event::Registry(None))));
    let mut error = guard(
        Box::new(Ended(Some(codeterm_neighbors::Error::Transport))),
        changes,
    );
    assert!(matches!(
        error.next().await,
        Err(codeterm_neighbors::Error::Transport)
    ));
}
