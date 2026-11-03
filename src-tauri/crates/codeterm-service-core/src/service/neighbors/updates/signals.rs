use codeterm_neighbors::{
    registry::{RegistryListener, RegistrySignal},
    Result,
};
use tokio::sync::{mpsc, watch};

pub(super) fn guard(
    mut listener: Box<dyn RegistryListener>,
    mut changes: watch::Receiver<u64>,
) -> Box<dyn RegistryListener> {
    let (sender, receiver) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = changes.changed() => {},
            _ = async move {
                loop {
                    let event = listener.next().await;
                    let ended = !matches!(&event, Ok(Some(_)));
                    if sender.send(event).await.is_err() || ended {
                        return;
                    }
                }
            } => {},
        }
    });
    Box::new(Signals { task, receiver })
}

struct Signals {
    task: tokio::task::JoinHandle<()>,
    receiver: mpsc::Receiver<Result<Option<RegistrySignal>>>,
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[async_trait::async_trait]
impl RegistryListener for Signals {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        self.receiver.recv().await.unwrap_or(Ok(None))
    }
}

#[cfg(test)]
mod tests;
