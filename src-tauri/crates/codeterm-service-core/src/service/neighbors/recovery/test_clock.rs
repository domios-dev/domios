use codeterm_neighbors::supervisor::RetryClock;
use std::{sync::Arc, time::Duration};

pub(in crate::service::neighbors) struct Clock {
    deadlines: tokio::sync::mpsc::UnboundedSender<Duration>,
    advance: tokio::sync::Semaphore,
}
impl Clock {
    pub fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Duration>) {
        let (deadlines, receiver) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(Self {
                deadlines,
                advance: tokio::sync::Semaphore::new(0),
            }),
            receiver,
        )
    }
    pub fn advance(&self) {
        self.advance.add_permits(1);
    }
}
#[async_trait::async_trait]
impl RetryClock for Clock {
    fn jitter(&self) -> u64 {
        500
    }
    async fn wait(&self, delay: Duration) {
        self.deadlines.send(delay).unwrap();
        self.advance.acquire().await.unwrap().forget();
    }
}
