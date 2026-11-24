use super::OutboundMessage;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const CAPACITY: usize = 128;
const MAX_AGE: Duration = Duration::from_secs(120);

type Entry = (Instant, OutboundMessage);
pub type OutboundSender = Arc<Mutex<Option<OutboundEnqueue>>>;

#[derive(Clone)]
pub struct OutboundEnqueue(mpsc::Sender<Entry>);

pub struct OutboundReceiver(mpsc::Receiver<Entry>);

pub fn outbound_channel() -> (OutboundEnqueue, OutboundReceiver) {
    let (sender, receiver) = mpsc::channel(CAPACITY);
    (OutboundEnqueue(sender), OutboundReceiver(receiver))
}

pub fn create_outbound_channel() -> (OutboundSender, OutboundReceiver) {
    let (sender, receiver) = outbound_channel();
    (Arc::new(Mutex::new(Some(sender))), receiver)
}

impl OutboundEnqueue {
    pub fn send(&self, message: OutboundMessage) -> Result<(), &'static str> {
        self.send_at(message, Instant::now())
    }

    fn send_at(&self, message: OutboundMessage, now: Instant) -> Result<(), &'static str> {
        self.0
            .try_send((now, message))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => "Telegram outbound queue is full",
                mpsc::error::TrySendError::Closed(_) => "Telegram outbound queue is closed",
            })
    }
}

impl OutboundReceiver {
    pub async fn recv(&mut self) -> Option<OutboundMessage> {
        while let Some(entry) = self.0.recv().await {
            if let Some(message) = take_at(entry, Instant::now()) {
                return Some(message);
            }
        }
        None
    }

    pub fn try_recv(&mut self) -> Result<OutboundMessage, mpsc::error::TryRecvError> {
        loop {
            if let Some(message) = take_at(self.0.try_recv()?, Instant::now()) {
                return Ok(message);
            }
        }
    }
}

fn take_at((queued_at, message): Entry, now: Instant) -> Option<OutboundMessage> {
    if now.saturating_duration_since(queued_at) < MAX_AGE {
        return Some(message);
    }
    if let Some(completion) = message.completion {
        let _ = completion.try_send(Err("Telegram outbound message expired in the queue".into()));
    }
    None
}

#[cfg(test)]
mod tests;
