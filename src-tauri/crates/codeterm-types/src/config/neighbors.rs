use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NeighborsConfig {
    pub enabled: bool,
}

impl Default for NeighborsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
