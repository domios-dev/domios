use serde::{Deserialize, Serialize};

/// Observe-only tab-state classification beside the structural startup nudge.
/// Section `[agent.tab_state_shadow]`; off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabStateShadowConfig {
    pub enabled: bool,
}
