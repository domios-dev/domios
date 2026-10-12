use crate::{AnalyticsSink, Config, DeliveryEvent, DeliveryResult, Project, SecretReader, Zone};
use std::time::Duration;

impl Zone {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Us => "https://api2.amplitude.com/2/httpapi",
            Self::Eu => "https://api.eu.amplitude.com/2/httpapi",
        }
    }
}

impl Project {
    pub fn secret_name(self) -> &'static str {
        match self {
            Self::Test => "domios.amplitude.test.api_key",
            Self::Prod => "domios.amplitude.prod.api_key",
        }
    }
}

pub struct Http {
    client: reqwest::blocking::Client,
    endpoint: String,
    project: Project,
    secrets: SecretReader,
}

impl Http {
    pub fn new(config: Config, secrets: SecretReader) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "analytics HTTP client unavailable")?;
        Ok(Self {
            client,
            endpoint: config.zone.endpoint().into(),
            project: config.project,
            secrets,
        })
    }

    #[cfg(test)]
    pub(crate) fn destination(&mut self, endpoint: String) {
        self.endpoint = endpoint;
    }
}

impl AnalyticsSink for Http {
    fn deliver(&mut self, batch: &[DeliveryEvent]) -> DeliveryResult {
        let Ok(key) = (self.secrets)(self.project.secret_name()) else {
            return DeliveryResult::Unavailable;
        };
        if key.is_empty() {
            return DeliveryResult::Unavailable;
        }
        let events: Result<Vec<_>, _> = batch
            .iter()
            .map(|event| {
                let mut value = serde_json::to_value(&event.event)?;
                value["insert_id"] = serde_json::json!(event.insert_id);
                value["device_id"] = serde_json::json!(event.device_id);
                value["time"] = serde_json::json!(event.time);
                if let Some(uid) = &event.user_id {
                    value["user_id"] = serde_json::json!(uid);
                }
                Ok::<_, serde_json::Error>(value)
            })
            .collect();
        let Ok(events) = events else {
            return DeliveryResult::InvalidPayload;
        };
        let code = self
            .client
            .post(&self.endpoint)
            .json(&serde_json::json!({"api_key": key, "events": events}))
            .send()
            .map(|reply| reply.status().as_u16())
            .unwrap_or(0);
        match code {
            200 => DeliveryResult::Delivered,
            400..=499 if code != 429 => DeliveryResult::InvalidPayload,
            _ => DeliveryResult::Retry,
        }
    }
}
