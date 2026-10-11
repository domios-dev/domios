use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginGrants {
    #[serde(flatten)]
    pub permissions: BTreeMap<String, bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub network_allow: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub project_write_roots: Vec<String>,
}
