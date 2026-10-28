use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NeighborsConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_store: Option<NeighborKeyStore>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
