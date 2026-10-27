use crate::Result;
use async_trait::async_trait;

pub struct Acceptance {
    pub event_id: String,
    pub friendship_id: String,
}

#[derive(Default)]
pub struct RegistrySignal {
    pub receiver_revision: Option<u64>,
    pub accepted: Vec<Acceptance>,
    pub removed: Vec<String>,
}

#[async_trait]
pub trait RegistryListener: Send {
    async fn next(&mut self) -> Result<Option<RegistrySignal>>;
}
