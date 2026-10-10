use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stored model defaults the daemon moved off a model its catalogue no longer
/// offers, keyed by the setting path (`agents.codex.default_model`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelDefaultsConfig {
    pub replaced: BTreeMap<String, ReplacedModelDefault>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplacedModelDefault {
    pub provider: String,
    pub from: String,
    pub to: String,
    pub reason: String,
}
