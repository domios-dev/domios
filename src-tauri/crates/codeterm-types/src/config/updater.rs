use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdaterConfig {
    pub channel: crate::update::Channel,
    pub allow_provider_updates_in_non_prod: bool,
    pub auto_update_provider_clis: bool,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            channel: Default::default(),
            allow_provider_updates_in_non_prod: false,
            auto_update_provider_clis: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_cli_auto_update_defaults_on_and_round_trips_opt_out() {
        let default: UpdaterConfig = serde_json::from_str("{}").unwrap();
        assert!(default.auto_update_provider_clis);
        let opted_out: UpdaterConfig =
            serde_json::from_str(r#"{"auto_update_provider_clis":false}"#).unwrap();
        let restored: UpdaterConfig =
            serde_json::from_value(serde_json::to_value(opted_out).unwrap()).unwrap();
        assert!(!restored.auto_update_provider_clis);
    }
}
