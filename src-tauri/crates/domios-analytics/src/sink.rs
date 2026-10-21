use crate::Event;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Serialize)]
pub struct DeliveryEvent {
    pub insert_id: Uuid,
    pub device_id: Uuid,
    pub user_id: Option<String>,
    pub time: u64,
    pub event: Event,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryResult {
    Delivered,
    InvalidPayload,
    Retry,
    Unavailable,
}

pub trait AnalyticsSink: Send {
    fn available(&self) -> bool {
        true
    }
    fn enabled(&self) -> bool {
        true
    }
    fn deliver(&mut self, events: &[DeliveryEvent]) -> DeliveryResult;
}
