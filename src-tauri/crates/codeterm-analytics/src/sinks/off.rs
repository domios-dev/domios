use crate::{AnalyticsSink, DeliveryEvent, DeliveryResult};
pub struct Off;
impl AnalyticsSink for Off {
    fn enabled(&self) -> bool {
        false
    }
    fn deliver(&mut self, _: &[DeliveryEvent]) -> DeliveryResult {
        DeliveryResult::Unavailable
    }
}
