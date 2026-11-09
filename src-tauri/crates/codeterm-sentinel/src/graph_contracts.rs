use crate::identity::Identity;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationScenario")]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub root: Identity,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationGraphSnapshot")]
pub struct Snapshot {
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub all_teams: bool,
    pub revision: String,
    pub scenarios: Vec<Scenario>,
    pub nodes: Vec<Value>,
    pub runs: Vec<Value>,
    pub blueprints: Vec<Value>,
    pub edges: Vec<Value>,
    pub truncated: bool,
    pub configured_fallback: Value,
    /// Committed Blueprints refused at load, with their typed reasons; they never run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::storage::quarantine::Quarantined>>", optional)]
    pub quarantined: Vec<crate::storage::quarantine::Quarantined>,
}
