use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabSleepConfig {
    pub enabled: bool,
    pub idle_minutes: u64,
}

impl Default for TabSleepConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_minutes: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TabSleepConfig;

    #[test]
    fn omitted_fields_keep_sleep_enabled_after_one_hour() {
        let config: TabSleepConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, TabSleepConfig::default());
        assert!(config.enabled);
        assert_eq!(config.idle_minutes, 60);
    }

    #[test]
    fn settings_round_trip_without_losing_the_off_switch() {
        let config = TabSleepConfig {
            enabled: false,
            idle_minutes: 120,
        };
        let serialized = serde_json::to_value(&config).unwrap();
        assert_eq!(
            serde_json::from_value::<TabSleepConfig>(serialized).unwrap(),
            config
        );
    }
}
