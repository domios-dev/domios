use codeterm_neighbors::{
    registry::{RegistryListener, RegistrySignal},
    Result,
};
use tokio::sync::{mpsc, watch};

pub(super) enum Event {
    PaidChanged,
    Registry(Option<RegistrySignal>),
}

pub(super) fn guard(
    mut listener: Box<dyn RegistryListener>,
    mut changes: watch::Receiver<u64>,
) -> Signals {
    let (sender, receiver) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        let changed = tokio::select! {
            biased;
            _ = changes.changed() => true,
            _ = async {
                loop {
                    let event = listener.next().await;
                    let ended = !matches!(&event, Ok(Some(_)));
                    if sender.send(event.map(Event::Registry)).await.is_err() || ended {
                        return;
                    }
                }
            } => false,
        };
        drop(listener);
        if changed {
            let _ = sender.send(Ok(Event::PaidChanged)).await;
        }
    });
    Signals { task, receiver }
}

pub(super) struct Signals {
    task: tokio::task::JoinHandle<()>,
    receiver: mpsc::Receiver<Result<Event>>,
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Signals {
    pub(super) async fn next(&mut self) -> Result<Event> {
        self.receiver
            .recv()
            .await
            .unwrap_or(Ok(Event::Registry(None)))
    }
}

#[cfg(test)]
mod tests;
