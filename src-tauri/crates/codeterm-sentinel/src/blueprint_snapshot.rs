use codeterm_blueprint_types::{
    blueprint_document::calls::projection::ExpandedCall, blueprint_journal::JournalState,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintState {
    Idle,
    Running,
    Held,
    Stopped,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintRunState {
    Idle,
    Running,
    Completed,
    Failed,
    Held,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintResultState {
    Waiting,
    Delivering,
    Delivered,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum WaitingStep {
    Waiting,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum BlueprintStepState {
    Waiting(WaitingStep),
    Recorded(JournalState),
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintSnapshotOwner")]
pub struct Owner {
    pub kind: String,
    pub tab_id: Option<String>,
    pub team_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintSnapshotResult")]
pub struct ResultView {
    pub target: ResultTarget,
    pub state: BlueprintResultState,
    pub reason: Option<String>,
    pub body_ref: Option<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintSnapshotResultTarget")]
pub struct ResultTarget {
    pub kind: String,
    pub id: Option<String>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintStep")]
pub struct Step {
    pub index: usize,
    pub name: String,
    pub kind: String,
    pub title: String,
    pub state: BlueprintStepState,
    pub outcome: Option<String>,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub result_summary: Option<String>,
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<StepSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_state: Option<Option<String>>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum StepSource {
    Github(codeterm_blueprint_types::blueprint_github::GithubProbe),
    Http(HttpSource),
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct HttpSource {
    pub url: String,
    pub expected_status: String,
    pub timeout_ms: u64,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintSnapshotTrigger")]
pub struct Trigger {
    pub r#type: String,
    pub summary: String,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct EventSubscription {
    pub state: String,
    pub delivered: u64,
    pub refused: u64,
    pub last_event_at_ms: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintRetainedProjection")]
pub struct Projection {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_nodes: Option<BTreeMap<String, PluginNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub historical_graph_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub graph_definition_version: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub call_graph: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expanded_calls: Option<Vec<ExpandedCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub nested_run: Option<NestedRun>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct NestedRun {
    pub id: Option<String>,
    pub definition_version: Option<u64>,
    pub nodes: Vec<JournalPreview>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum PluginNode {
    Available(AvailablePluginNode),
    Unavailable { available: bool },
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct AvailablePluginNode {
    pub available: bool,
    pub title: String,
    pub plugin: String,
    pub description: String,
    pub inputs: BTreeMap<String, codeterm_plugin::blueprint_step::StepPort>,
    pub outputs: BTreeMap<String, codeterm_plugin::blueprint_step::StepPort>,
    pub outcomes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintRunSnapshot")]
pub struct Run {
    pub id: Option<String>,
    pub state: BlueprintRunState,
    pub closed: bool,
    pub started_at_ms: Option<u64>,
    pub last_event_at_ms: Option<u64>,
    pub current_step: Option<u32>,
    pub steps: Vec<Step>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<ResultView>,
    #[serde(flatten)]
    pub projection: Projection,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintSnapshot")]
pub struct Snapshot {
    pub id: String,
    pub name: Option<String>,
    pub note: Option<String>,
    pub stopped_at: Option<u64>,
    pub deletes_at: Option<u64>,
    pub owner: Owner,
    pub state: BlueprintState,
    pub run_id: Option<String>,
    pub current_step: Option<u32>,
    pub last_event_at_ms: Option<u64>,
    pub steps: Vec<Step>,
    pub runs: Vec<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<ResultView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trigger: Option<Trigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub held_reason: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_hold: Option<crate::admission::PluginHold>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub event_subscription: Option<EventSubscription>,
    #[serde(flatten)]
    pub projection: Projection,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "BlueprintJournalPreview")]
pub struct JournalPreview {
    #[serde(flatten)]
    pub evidence: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub entry_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub step_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub step_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<JournalState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub started_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ended_at_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result_summary: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body_ref: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub call_stack: Option<Vec<codeterm_blueprint_types::blueprint_document::calls::CallFrame>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_step_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub definition_version: Option<u64>,
}
