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
    let listener = guard(
        Box::new(Held {
            ready: Some(ready),
            closed: Some(closed),
        }),
        changes,
    );
    started.await.unwrap();
    change.send_modify(|revision| *revision += 1);
    ended.await.unwrap();
    drop(listener);
}
