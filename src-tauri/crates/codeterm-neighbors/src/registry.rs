use crate::Result;
use async_trait::async_trait;

pub struct Acceptance {
    pub event_id: String,
    pub friendship_id: String,
}

#[derive(Default)]
pub struct RegistrySignal {
    pub accepted: Vec<Acceptance>,
}

#[async_trait]
pub trait RegistryListener: Send {
    async fn next(&mut self) -> Result<Option<RegistrySignal>>;
}
