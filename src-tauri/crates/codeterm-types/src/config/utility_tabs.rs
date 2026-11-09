use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct UtilityTabsConfig {
    pub max_per_team: UtilityTabLimits,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct UtilityTabLimits {
    pub build: Option<usize>,
    pub dev: Option<usize>,
    pub qa: Option<usize>,
    pub deploy: Option<usize>,
    pub logs: Option<usize>,
    pub db: Option<usize>,
    pub shell: Option<usize>,
}
