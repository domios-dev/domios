use crate::Result;
use async_trait::async_trait;

pub struct Acceptance {
    pub event_id: String,
    pub friendship_id: String,
}

pub struct DeliverySignal {
    pub reason: Option<crate::wire::DeliveryReason>,
    pub friendship_id: String,
    pub message_id: String,
    pub status: crate::wire::DeliveryStatus,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadSignal {
    pub friendship_id: String,
    pub message_id: String,
    pub read_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseSignal {
    pub friendship_id: String,
    pub paused: bool,
}

#[derive(Default)]
pub struct RegistrySignal {
    pub revision: Option<u64>,
    pub delivery: Option<DeliverySignal>,
    pub read: Option<ReadSignal>,
    pub pause: Option<PauseSignal>,
    pub ignored: bool,
    pub receiver_revision: Option<u64>,
    pub receiver_friendship: Option<String>,
    pub accepted: Vec<Acceptance>,
    pub removed: Vec<String>,
}

#[async_trait]
pub trait RegistryListener: Send {
    async fn next(&mut self) -> Result<Option<RegistrySignal>>;
}
