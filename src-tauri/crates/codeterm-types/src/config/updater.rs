use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct UpdaterConfig {
    pub channel: crate::update::Channel,
    pub allow_provider_updates_in_non_prod: bool,
    pub auto_update_provider_clis: bool,
    #[ts(skip)]
    pub require_idle_for_app_update: bool,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            channel: Default::default(),
            allow_provider_updates_in_non_prod: false,
            auto_update_provider_clis: true,
            require_idle_for_app_update: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_update_accepts_busy_work_by_default_and_preserves_idle_only_opt_out() {
        let default: UpdaterConfig = serde_json::from_str("{}").unwrap();
        assert!(!default.require_idle_for_app_update);
        let opted_out: UpdaterConfig =
            serde_json::from_str(r#"{"require_idle_for_app_update":true}"#).unwrap();
        let restored: UpdaterConfig =
            serde_json::from_value(serde_json::to_value(opted_out).unwrap()).unwrap();
        assert!(restored.require_idle_for_app_update);
    }

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
