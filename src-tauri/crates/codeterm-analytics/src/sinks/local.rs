use super::amplitude::{resolve_key, Http};
use crate::{AnalyticsSink, Config, DeliveryEvent, DeliveryResult, SecretReader};
use std::sync::OnceLock;

pub struct Local {
    config: Config,
    build_key: Option<&'static str>,
    secrets: SecretReader,
    vendor: Option<Http>,
    key: OnceLock<Option<String>>,
}
impl Local {
    pub fn new(config: Config, secrets: SecretReader) -> Self {
        Self::with_build_key(
            config,
            secrets,
            crate::build_config::amplitude_key(config.project),
        )
    }
    fn with_build_key(
        config: Config,
        secrets: SecretReader,
        build_key: Option<&'static str>,
    ) -> Self {
        Self {
            config,
            build_key,
            secrets,
            vendor: None,
            key: OnceLock::new(),
        }
    }
    fn key(&self) -> Option<&String> {
        self.key
            .get_or_init(|| resolve_key(self.build_key, self.config.project, &self.secrets))
            .as_ref()
    }
}
impl AnalyticsSink for Local {
    fn available(&self) -> bool {
        self.key().is_some()
    }
    fn deliver(&mut self, events: &[DeliveryEvent]) -> DeliveryResult {
        if !self.available() {
            return DeliveryResult::Unavailable;
        }
        if self.vendor.is_none() {
            let Ok(vendor) = Http::with_key(self.config, self.key().cloned()) else {
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

#[cfg(test)]
mod tests;
