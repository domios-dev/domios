use crate::{AnalyticsSink, DeliveryEvent, DeliveryResult};
pub struct Console;
impl AnalyticsSink for Console {
    fn deliver(&mut self, events: &[DeliveryEvent]) -> DeliveryResult {
        for event in events {
            log::debug!(
                "[analytics] {}",
                serde_json::to_string(&event.event).unwrap_or_default()
            );
        }
        DeliveryResult::Delivered
    }
}
