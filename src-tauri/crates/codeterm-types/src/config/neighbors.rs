use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct NeighborsConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key_store: Option<NeighborKeyStore>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
pub enum NeighborKeyStore {
    Os,
    File,
}

impl Default for NeighborsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            key_store: None,
        }
    }
}
