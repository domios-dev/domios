use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct HostError {
    pub error: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub struct HostWorkerStartOptions {
    pub task: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostWorkspaceOptions {
    pub name: String,
    pub external_root: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostWorkspaceResult {
    pub workspace_id: String,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostJobStart {
    pub job_id: String,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostSessionResult {
    pub session_id: String,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostTicketResult {
    pub ticket: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub struct HostAgentSpawnOptions {
    pub task: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub key: Option<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostWorkerPoll {
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostAgentPoll {
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Deserialize, ts_rs::TS)]
pub struct HostMemSearchOptions {
    pub query: String,
    pub k: Option<usize>,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostMemHit {
    pub id: String,
    pub text: String,
    pub score: f64,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostMemSearchResult {
    pub hits: Vec<HostMemHit>,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostRunStart {
    pub run: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepRunState {
    Running,
    Done { text: String },
    Failed { reason: String },
    Canceled,
    Unknown,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceCallContext<'a> {
    pub surface: &'a str,
    pub instance: &'a str,
    pub tab: &'a str,
    pub owner: &'a str,
    pub actor: &'a str,
    pub actor_label: String,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostSurfaceResult {
    pub result: String,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostOk {
    pub ok: bool,
}

#[derive(Serialize, ts_rs::TS)]
pub struct HostRefusal {
    #[serde(flatten)]
    pub error: HostError,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, ts_rs::TS)]
pub struct HostSurfaceArguments {
    pub args: Vec<String>,
    pub of: Option<String>,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct WatcherTickInput {
    pub tick: u64,
    pub now_ms: u64,
    pub state: serde_json::Value,
    pub observations: serde_json::Value,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum CoreMachineEngine {
    Machine {
        charter: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        tick_interval_secs: Option<u64>,
    },
}

#[cfg(test)]
mod tests;
