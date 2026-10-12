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
    build_key: Option<String>,
}

impl Http {
    pub(crate) fn available_for(config: Config, secrets: &SecretReader) -> bool {
        resolve_key(
            crate::build_config::amplitude_key(config.project),
            config.project,
            secrets,
        )
        .is_some()
    }

    pub fn new(config: Config, secrets: SecretReader) -> Result<Self, String> {
        Self::with_build_key(
            config,
            secrets,
            crate::build_config::amplitude_key(config.project),
        )
    }
    pub(crate) fn with_build_key(
        config: Config,
        secrets: SecretReader,
        build_key: Option<&str>,
    ) -> Result<Self, String> {
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
            build_key: build_key.map(str::to_owned),
        })
    }

    #[cfg(test)]
    pub(crate) fn destination(&mut self, endpoint: String) {
        self.endpoint = endpoint;
    }
}

impl AnalyticsSink for Http {
    fn available(&self) -> bool {
        resolve_key(self.build_key.as_deref(), self.project, &self.secrets).is_some()
    }

    fn deliver(&mut self, batch: &[DeliveryEvent]) -> DeliveryResult {
        let Some(key) = resolve_key(self.build_key.as_deref(), self.project, &self.secrets) else {
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

fn resolve_key(
    build_key: Option<&str>,
    project: Project,
    secrets: &SecretReader,
) -> Option<String> {
    build_key
        .filter(|key| !key.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            secrets(project.secret_name())
                .ok()
                .filter(|key| !key.trim().is_empty())
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_precedence_build_then_secret_store() {
        let never_read: SecretReader =
            std::sync::Arc::new(|_| panic!("build key must bypass secret store"));
        assert_eq!(
            resolve_key(Some("build-key"), Project::Test, &never_read).as_deref(),
            Some("build-key")
        );
        let secrets: SecretReader = std::sync::Arc::new(|_| Ok("store-key".into()));
        assert_eq!(
            resolve_key(Some("build-key"), Project::Test, &secrets).as_deref(),
            Some("build-key")
        );
        assert_eq!(
            resolve_key(None, Project::Prod, &secrets).as_deref(),
            Some("store-key")
        );
        assert_eq!(
            resolve_key(Some(""), Project::Prod, &secrets).as_deref(),
            Some("store-key")
        );
        let missing: SecretReader = std::sync::Arc::new(|_| Err("missing".into()));
        assert_eq!(resolve_key(None, Project::Test, &missing), None);
    }
}
