use crate::Result;
use async_trait::async_trait;

pub struct Acceptance {
    pub event_id: String,
    pub friendship_id: String,
}

pub struct DeliverySignal {
    pub friendship_id: String,
    pub message_id: String,
    pub status: crate::wire::DeliveryStatus,
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
    pub pause: Option<PauseSignal>,
    pub ignored: bool,
    pub receiver_revision: Option<u64>,
    pub accepted: Vec<Acceptance>,
    pub removed: Vec<String>,
}

#[async_trait]
pub trait RegistryListener: Send {
    async fn next(&mut self) -> Result<Option<RegistrySignal>>;
}
