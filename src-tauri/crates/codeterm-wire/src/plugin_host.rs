#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverTargetView {
    pub plugin_id: String,
    pub tab_id: Option<String>,
    pub live: bool,
}

#[derive(serde::Deserialize, ts_rs::TS)]
pub struct ProgressOpts {
    pub label: String,
    #[serde(default)]
    pub percent: Option<f32>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepAgentStartOptions {
    pub prompt: String,
    pub cwd: String,
    pub model: Option<String>,
    pub timeout_ms: u64,
    pub sandbox: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ViewIdentity {
    pub surface: String,
    pub instance: String,
    pub tab: String,
}
mod compute;
pub use compute::*;
