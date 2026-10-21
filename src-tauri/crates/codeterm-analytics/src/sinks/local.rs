use super::amplitude::Http;
use crate::{AnalyticsSink, Config, DeliveryEvent, DeliveryResult, SecretReader};

pub struct Local {
    config: Config,
    secrets: SecretReader,
    vendor: Option<Http>,
}
impl Local {
    pub fn new(config: Config, secrets: SecretReader) -> Self {
        Self {
            config,
            secrets,
            vendor: None,
        }
    }
}
impl AnalyticsSink for Local {
    fn available(&self) -> bool {
        Http::available_for(self.config, &self.secrets)
    }
    fn deliver(&mut self, events: &[DeliveryEvent]) -> DeliveryResult {
        if !self.available() {
            return DeliveryResult::Unavailable;
        }
        if self.vendor.is_none() {
            let Ok(vendor) = Http::new(self.config, self.secrets.clone()) else {
                return DeliveryResult::Unavailable;
            };
            self.vendor = Some(vendor);
        }
        match &mut self.vendor {
            Some(vendor) => vendor.deliver(events),
            None => DeliveryResult::Unavailable,
        }
    }
}
