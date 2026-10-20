use serde::{Deserialize, Serialize};

/// A project settings file the provider asks every Worker lane to patch, for
/// example to keep its own language-server plugin from building in the lane.
/// `path` is lane-relative; `patch` is a JSON object merged into the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaneSetting {
    pub path: String,
    pub patch: serde_json::Value,
}
