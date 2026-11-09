use crate::identity::Identity;
use serde::Serialize;

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
    pub nodes: Vec<Node>,
    pub runs: Vec<Run>,
    pub blueprints: Vec<crate::blueprint_snapshot::Snapshot>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
    pub configured_fallback: Option<ConfiguredFallback>,
    /// Committed Blueprints refused at load, with their typed reasons; they never run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::storage::quarantine::Quarantined>>", optional)]
    pub quarantined: Vec<crate::storage::quarantine::Quarantined>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationHandlerResult")]
pub struct HandlerResult {
    pub status: Option<String>,
    pub execution_id: Option<String>,
    pub accepted_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_result: Option<crate::command::ResultMetadata>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationHandler")]
pub struct Handler {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout_ms: Option<u64>,
    pub target: Option<Identity>,
    pub availability: Availability,
    pub last_result: Option<HandlerResult>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AutomationAvailability")]
pub enum Availability {
    Available,
    Missing,
    #[default]
    Unavailable,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationOutcome")]
pub struct Outcome {
    pub bucket: String,
    pub handlers: Vec<Handler>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationExecutor")]
pub struct Executor {
    pub kind: Option<String>,
    pub selection: String,
    pub provider_id: Option<String>,
    pub adapter_id: Option<String>,
    pub model: Option<String>,
    pub configured_model: Option<String>,
    pub reported_model: Option<String>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct AgentBlueprintDefinition {
    pub source: String,
    pub definition_id: String,
    pub creator_tab_id: String,
    pub definition_version: u64,
    pub lineage: Vec<DefinitionLineage>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DefinitionLineage {
    pub version: u64,
    pub replaced_version: u64,
    pub author_tab_id: String,
    pub cause_transition_id: Option<String>,
    pub at: u64,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationEdge")]
pub struct Edge {
    pub id: String,
    pub source: Identity,
    pub target: Identity,
    pub bucket: String,
    pub handler_id: String,
    pub handler_type: String,
    pub result: Option<HandlerResult>,
    pub evidence: EdgeEvidence,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "AutomationEdgeEvidence")]
pub enum EdgeEvidence {
    Configured { route_id: String },
    Observed { execution_id: String },
}

#[derive(Clone, Debug, Default, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationDecision")]
pub struct DecisionProjection {
    pub decided_by: Option<crate::provenance::DecidedBy>,
    pub adapter_id: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationTransitionHandler")]
pub struct TransitionHandler {
    pub handler_id: String,
    #[serde(flatten)]
    pub result: HandlerResult,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationTransition")]
pub struct Transition {
    pub causal_run: Option<codeterm_blueprint_types::blueprint_run::Context>,
    pub id: String,
    pub bucket: String,
    pub accepted_at: u64,
    pub routes_current: bool,
    pub pending_handlers: bool,
    pub handlers: Vec<TransitionHandler>,
    #[serde(flatten)]
    pub decision: DecisionProjection,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationRun")]
pub struct Run {
    pub id: String,
    pub root_node: String,
    pub root_transition: String,
    pub started_at: u64,
    pub state: RunState,
    pub incomplete: bool,
    pub transitions: Vec<RunTransition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AutomationRunState")]
pub enum RunState {
    Running,
    Completed,
    Unconfirmed,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationConversation")]
pub struct Conversation {
    pub enabled: bool,
    pub exchanges: usize,
    pub actions: usize,
    pub safety_limit: usize,
    pub rate_limit_per_minute: usize,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationRunTransition")]
pub struct RunTransition {
    pub node: Identity,
    pub transition_id: String,
    pub parent: Option<codeterm_blueprint_types::blueprint_run::Parent>,
    pub conversation: Option<Conversation>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationLastResult")]
pub struct LastResult {
    pub bucket: String,
    pub accepted_at: u64,
    pub handlers: Vec<TransitionHandler>,
    #[serde(flatten)]
    pub decision: DecisionProjection,
}

#[derive(Clone, Debug, Default, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationStop")]
pub struct Stop {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationNode")]
pub struct Node {
    pub identity: Identity,
    pub name: String,
    pub purpose: Option<String>,
    pub state: Option<String>,
    pub availability: Availability,
    pub hidden: Option<bool>,
    pub schedule: Option<String>,
    pub cadence_ms: Option<u64>,
    pub expires_at: Option<u64>,
    pub outcomes: Vec<Outcome>,
    pub last_result: Option<LastResult>,
    pub stop: Stop,
    pub executor: Option<Executor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub held_reason: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stopped_at: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub deletes_at: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cron_run_state: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversation: Option<Conversation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub solution_success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<Detail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<SourceDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blueprint_definition: Option<AgentBlueprintDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub history: Option<Vec<Transition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub history_truncated: Option<bool>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(rename = "AutomationSourceDescriptor")]
pub enum SourceDescriptor {
    Pipeline,
    Probe,
    Input {
        kind: String,
        ref_type: String,
        display_name: String,
        convert_to: Option<String>,
    },
    Observe {
        kind: String,
    },
    Terminal {
        kind: String,
        pattern: Option<String>,
        idle_ms: Option<u64>,
        max_payload_bytes: usize,
    },
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationDetail")]
pub struct Detail {
    pub version: u32,
    pub scope: DetailScope,
    pub origin: String,
    pub creator: Option<DetailCreator>,
    pub team: Option<DetailTeam>,
    pub observe_target: Option<codeterm_blueprint_types::blueprint_watch::Observation>,
    pub next_check_at_ms: Option<u64>,
    pub definition: Definition,
    pub decisions: DetailDecisions,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runs_since_transition: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_probe_at: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe_state: Option<Option<String>>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DetailScope {
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub node_id: String,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DetailCreator {
    pub tab_id: String,
    pub name: Option<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DetailTeam {
    pub id: String,
    pub name: Option<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DetailDecisions {
    pub history_complete: bool,
    pub adapter_used: Option<bool>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(rename = "AutomationDefinition")]
pub enum Definition {
    Unavailable { unavailable: String },
    Available(AvailableDefinition),
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct AvailableDefinition {
    pub probe: Option<String>,
    pub probe_truncated: bool,
    pub model_classification: bool,
    pub decision_kind: String,
    pub command_language: String,
    pub language: String,
    pub question: String,
    pub buckets: Vec<DefinitionBucket>,
    pub commands: Vec<DefinitionCommand>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DefinitionBucket {
    pub name: String,
    pub pattern: Option<String>,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct DefinitionCommand {
    pub bucket: String,
    pub handler_id: String,
    pub label: String,
    pub command: String,
    pub command_truncated: bool,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(rename = "AutomationConfiguredFallback")]
pub struct ConfiguredFallback {
    pub adapter_id: String,
    pub model: Option<String>,
}
