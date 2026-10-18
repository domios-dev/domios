use std::sync::Arc;

use codeterm_events::{EventBroadcaster, SeqEvent};
use tokio::sync::{broadcast::error::RecvError, watch};

pub struct Subscription {
    task: tokio::task::JoinHandle<()>,
    acknowledged: watch::Receiver<Result<u64, String>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Subscription {
    pub async fn acknowledged(&self, seq: u64) -> Result<(), String> {
        let mut receiver = self.acknowledged.clone();
        loop {
            match receiver.borrow_and_update().clone() {
                Ok(applied) if applied >= seq => return Ok(()),
                Err(error) => return Err(error),
                _ => {}
            }
            receiver
                .changed()
                .await
                .map_err(|error| error.to_string())?;
        }
    }
}

pub fn subscribe(
    events: Arc<EventBroadcaster>,
    apply: impl Fn(&SeqEvent) + Send + Sync + 'static,
    failed: impl Fn(&str) + Send + Sync + 'static,
) -> Subscription {
    let (mut receiver, mut cursor) = events.subscribe_from_now();
    let (acknowledge, acknowledged) = watch::channel(Ok(cursor));
    let task = codeterm_service_core::async_runtime::spawn(async move {
        loop {
            let batch = match receiver.recv().await {
                Ok(event) => vec![event],
                Err(RecvError::Closed) => return,
                Err(RecvError::Lagged(_)) => {
                    let snapshot = events.snapshot_ring();
                    if snapshot
                        .entries
                        .first()
                        .is_some_and(|event| event.seq > cursor + 1)
                    {
                        let error = format!("automation event history lost after {cursor}");
                        failed(&error);
                        acknowledge.send_replace(Err(error));
                        return;
                    }
                    snapshot
                        .entries
                        .iter()
                        .map(|event| (**event).clone())
                        .collect()
                }
            };
            for event in batch {
                if event.seq <= cursor {
                    continue;
                }
                apply(&event);
                cursor = event.seq;
                acknowledge.send_replace(Ok(cursor));
            }
        }
    });
    Subscription { task, acknowledged }
}

pub fn payload(event: &SeqEvent) -> serde_json::Value {
    match &event.event {
        codeterm_events::SystemEvent::Generic { data, .. } => data.clone(),
        event => serde_json::to_value(event).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_split_contract_subscription_replays_lag_without_duplicates() {
        let events = Arc::new(EventBroadcaster::new());
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let entered = parking_lot::Mutex::new(Some(entered));
        let (release, wait) = std::sync::mpsc::channel();
        let wait = parking_lot::Mutex::new(wait);
        let subscription = subscribe(
            events.clone(),
            move |event| {
                recorded.lock().push(event.seq);
                if let Some(entered) = entered.lock().take() {
                    entered.send(()).unwrap();
                    wait.lock()
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .unwrap();
                }
            },
            |error| panic!("{error}"),
        );
        events.publish(codeterm_events::SystemEvent::MuxStateChanged);
        tokio::time::timeout(std::time::Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        for _ in 0..codeterm_events::SYSTEM_BUS_CAPACITY + 1 {
            events.publish_generic(
                "prompt-state-changed",
                serde_json::json!({"tab_id":"bound"}),
            );
        }
        let last = events.current_seq() - 1;
        release.send(()).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            subscription.acknowledged(last),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*seen.lock(), (1..=last).collect::<Vec<_>>());
    }
}
