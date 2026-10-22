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
    key: Option<String>,
}

impl Http {
    #[cfg(test)]
    pub(crate) fn with_build_key(
        config: Config,
        secrets: SecretReader,
        build_key: Option<&str>,
    ) -> Result<Self, String> {
        Self::with_key(config, resolve_key(build_key, config.project, &secrets))
    }
    pub(crate) fn with_key(config: Config, key: Option<String>) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "analytics HTTP client unavailable")?;
        Ok(Self {
            client,
            endpoint: config.zone.endpoint().into(),
            key,
        })
    }

    #[cfg(test)]
    pub(crate) fn destination(&mut self, endpoint: String) {
        self.endpoint = endpoint;
    }
}

impl AnalyticsSink for Http {
    fn available(&self) -> bool {
        self.key.is_some()
    }

    fn deliver(&mut self, batch: &[DeliveryEvent]) -> DeliveryResult {
        let Some(key) = self.key.as_deref() else {
            return DeliveryResult::Unavailable;
        };
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

pub(crate) fn resolve_key(
    build_key: Option<&str>,
    project: Project,
    secrets: &SecretReader,
) -> Option<String> {
    secrets(project.secret_name())
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            build_key
                .filter(|key| !key.trim().is_empty())
                .map(str::to_owned)
        })
}
#[cfg(test)]
mod tests;
