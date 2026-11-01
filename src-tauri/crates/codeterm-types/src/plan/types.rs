use super::execution::TaskExecutionIntent;
use super::external_refs::ExternalRef;
use super::review::{
    PlanEvidenceReporterGrant, ReviewAuditEntry, ReviewEvidence, ReviewOverride, ReviewPolicy,
};
use super::{
    TaskCompetitionMetadata, TaskExecutionAttempt, TaskInterruptionState, TaskLaneMetadata,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode {
    #[default]
    Planning,
    Approved,
    Execution,
    Paused,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    InReview,
    Blocked,
    Paused,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStage {
    #[default]
    Review,
    Qa,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QaState {
    Queued,
    Running,
}

impl TaskStatus {
    pub fn key(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::InReview => "in_review",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Paused => "paused",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Cancelled)
    }

    // Paused work is deliberately excluded from the heartbeat open-work set.
    pub fn counts_as_open_work(&self) -> bool {
        matches!(
            self,
            TaskStatus::Pending
                | TaskStatus::InProgress
                | TaskStatus::InReview
                | TaskStatus::Blocked
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Running,
    Done,
    Exhausted,
    Stopped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementCanvas {
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorRef {
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub title: Option<String>,
}

impl OrchestratorRef {
    pub fn from_tab(p: &str) -> Self {
        Self {
            tab_id: Some(p.into()),
            ..Default::default()
        }
    }
}

pub type AgentRef = OrchestratorRef;

/// Planner binding on a plan. None = unset; Waived = explicitly solo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannerRef {
    Tab {
        tab_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Waived {
        note: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRequestState {
    Queued,
    Accepted,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerDeliveryPhase {
    #[default]
    PreSend,
    SendStarted,
    ProviderAccepted,
    PlannerAcked,
    DeliveryUncertain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerDeliveryTransport {
    PtyAgent,
    ChatBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerInboxEntry {
    pub request_id: String,
    #[serde(default)]
    pub phase: PlannerDeliveryPhase,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub binding_nonce: Option<String>,
    #[serde(default)]
    pub transport: Option<PlannerDeliveryTransport>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannerRequest {
    pub request_id: String,
    pub payload: String,
    #[serde(default)]
    pub payload_blob: Option<String>,
    #[serde(default)]
    pub caller_tab_id: Option<String>,
    #[serde(default)]
    pub caller_session_id: Option<String>,
    pub state: PlannerRequestState,
    pub created_at: u64,
    #[serde(default)]
    pub accepted_at: Option<u64>,
    #[serde(default)]
    pub lease_expires_at: Option<u64>,
    #[serde(default)]
    pub delivery_attempts: u32,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub failed_at: Option<u64>,
    #[serde(default)]
    pub delivered_to_tab_id: Option<String>,
    #[serde(default)]
    pub delivered_to_session_id: Option<String>,
    #[serde(default)]
    pub delivered_with_binding_nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferRecord {
    pub from: OrchestratorRef,
    pub to: OrchestratorRef,
    pub ts: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanLifecycleEvent {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    pub at: u64,
    pub author: OrchestratorRef,
}

/// Append-only audit record of an authorized Orchestrator waiving one or more
/// pending regression gates so frontier dispatch may proceed. A waiver only
/// applies while `ts >= group completion` — reopening/reclosing a group re-arms
/// its gate past the waiver, so an override never silently outlives its scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegressionGateOverride {
    pub actor: AgentRef,
    pub reason: String,
    pub gates: Vec<String>,
    pub ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MilestoneKind {
    pub key: String,
    pub label: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskGroup {
    pub key: String,
    pub label: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TagFacetValue {
    pub v: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TagFacet {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color_lane: bool,
    #[serde(default)]
    pub values: Vec<TagFacetValue>,
}

pub fn tag_facets_from_value(value: &serde_json::Value) -> Result<Vec<TagFacet>, String> {
    match value {
        serde_json::Value::String(raw) => tag_facets_from_string(raw),
        serde_json::Value::Array(_) => {
            serde_json::from_value(value.clone()).map_err(|e| format!("invalid tag_facets: {}", e))
        }
        serde_json::Value::Object(_) => serde_json::from_value::<TagFacet>(value.clone())
            .map(|facet| vec![facet])
            .map_err(|e| format!("invalid tag_facets: {}", e)),
        _ => Err("tag_facets must be an array, object, or JSON string".into()),
    }
}

pub fn tag_facets_from_string(raw: &str) -> Result<Vec<TagFacet>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("invalid tag_facets JSON: {}", e))?;
        return tag_facets_from_value(&value);
    }
    compact_tag_facets(trimmed)
}

fn compact_tag_facets(raw: &str) -> Result<Vec<TagFacet>, String> {
    raw.split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(compact_tag_facet)
        .collect()
}

fn compact_tag_facet(raw: &str) -> Result<TagFacet, String> {
    let mut parts = raw.splitn(3, ':');
    let key = parts.next().unwrap_or_default().trim();
    let label = parts.next().unwrap_or(key).trim();
    let values_raw = parts
        .next()
        .ok_or_else(|| format!("facet '{raw}' must use key:label:value[=label][#color]|..."))?;
    if key.is_empty() {
        return Err("facet key cannot be empty".into());
    }
    let values = values_raw
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(compact_tag_facet_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TagFacet {
        key: key.into(),
        label: Some(label.into()),
        color_lane: true,
        values,
    })
}

fn compact_tag_facet_value(raw: &str) -> Result<TagFacetValue, String> {
    let (main, color) = raw.split_once('#').unwrap_or((raw, ""));
    let (value, label) = main.split_once('=').unwrap_or((main, main));
    let value = value.trim();
    if value.is_empty() {
        return Err("facet value cannot be empty".into());
    }
    let label = label.trim();
    Ok(TagFacetValue {
        v: value.into(),
        label: if label.is_empty() {
            None
        } else {
            Some(label.into())
        },
        color: if color.trim().is_empty() {
            None
        } else {
            Some(color.trim().into())
        },
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    pub vector: String,
    #[serde(default)]
    pub vision: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub postulates: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatLearning {
    pub id: String,
    pub text: String,
    pub ts: u64,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorHeartbeat {
    #[serde(default)]
    pub current_focus: Option<String>,
    #[serde(default)]
    pub active_lanes: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub learnings: Vec<HeartbeatLearning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    #[serde(alias = "name")]
    pub title: String,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub subsections: Vec<Section>,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AttachmentRef {
    pub id: String,
    pub name: String,
    pub path: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone {
    pub id: String,
    pub from: AgentRef,
    pub kind: String,
    #[serde(default)]
    pub text: String,
    pub ts: u64,
    #[serde(default)]
    pub links: Vec<String>,
    pub loop_id: Option<String>,
    pub iteration: Option<u32>,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    #[serde(default)]
    pub files: super::TaskFiles,
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    pub model_rationale: Option<String>,
    #[serde(default)]
    pub personality: Option<String>,
    #[serde(default)]
    pub personality_provenance: Option<IdentityProvenance>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub external_refs: Vec<ExternalRef>,
    pub test_plan: Option<String>,
    #[serde(default)]
    pub qa_plan: Option<String>,
    pub correctness: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub review_stage: ReviewStage,
    #[serde(default)]
    pub qa_state: Option<QaState>,
    #[serde(default)]
    pub review_round: u32,
    #[serde(default)]
    pub review_oracle: Option<String>,
    #[serde(default)]
    pub review_evidence: Vec<ReviewEvidence>,
    #[serde(default)]
    pub review_override: Option<ReviewOverride>,
    #[serde(default)]
    pub review_audit: Vec<ReviewAuditEntry>,
    pub status_label: Option<String>,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub status_changed_at: u64,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub section_ids: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub role_guidance: Option<String>,
    #[serde(default)]
    pub executor_role: Option<String>,
    #[serde(default)]
    pub competition: Option<TaskCompetitionMetadata>,
    pub intent: Option<String>,
    #[serde(default)]
    pub execution_intent: TaskExecutionIntent,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default, alias = "dependency_task_ids")]
    pub depends_on: Vec<String>,
    pub assigned_tab_id: Option<String>,
    #[serde(default)]
    pub lane: TaskLaneMetadata,
    #[serde(default)]
    pub taken_by_role: Option<String>,
    pub peer: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub execution_attempts: Vec<TaskExecutionAttempt>,
    #[serde(default)]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub interruption: TaskInterruptionState,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityProvenance {
    UserSupplied,
    Suggested,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopSpec {
    pub id: String,
    pub target: String,
    pub budget: u32,
    #[serde(default)]
    pub count: u32,
    pub stop_when: Option<String>,
    pub status: LoopStatus,
    #[serde(default)]
    pub stagnation: u32,
    #[serde(default)]
    pub deviation: Option<String>,
    #[serde(default)]
    pub last_iteration_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanvasElement {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub canvas: ElementCanvas,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub pan_x: f64,
    pub pan_y: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanCanvas {
    pub viewport: Option<Viewport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecLink {
    pub spec_id: String,
    pub spec_section_id: String,
    pub plan_section_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EffortLinkRel {
    Follows,
    Supersedes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffortLink {
    pub to: String,
    pub rel: EffortLinkRel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spec {
    pub id: String,
    #[serde(default)]
    pub plan_id: Option<String>,
    pub title: String,
    pub project_path: String,
    #[serde(default)]
    pub canonical_project_path: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub sections: Vec<Section>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub canvas: PlanCanvas,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub sections: Vec<Section>,
    pub status: PlanStatus,
    #[serde(default)]
    pub mode: PlanMode,
    #[serde(default)]
    pub paused_from_mode: Option<PlanMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pause_operations: Vec<super::pause::PlanPauseOperation>,
    pub owner: OrchestratorRef,
    /// Optional 1:1 team binding; None keeps legacy plans teamless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub project_path: String,
    #[serde(default)]
    pub canonical_project_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub milestone_kinds: Vec<MilestoneKind>,
    #[serde(default)]
    pub loops: Vec<LoopSpec>,
    #[serde(default)]
    pub elements: Vec<CanvasElement>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub task_groups: Vec<TaskGroup>,
    #[serde(default)]
    pub tag_facets: Vec<TagFacet>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub spec_refs: Vec<String>,
    #[serde(default)]
    pub spec_links: Vec<SpecLink>,
    #[serde(default)]
    pub primary_spec_id: Option<String>,
    #[serde(default)]
    pub lineage: Vec<EffortLink>,
    #[serde(default)]
    pub goal: Option<Goal>,
    #[serde(default)]
    pub heartbeat: OrchestratorHeartbeat,
    #[serde(default)]
    pub control_loop: super::ControlLoop,
    #[serde(default)]
    pub review_policy: ReviewPolicy,
    #[serde(default)]
    pub evidence_reporters: Vec<PlanEvidenceReporterGrant>,
    #[serde(default)]
    pub canvas: PlanCanvas,
    #[serde(default)]
    pub owner_history: Vec<TransferRecord>,
    #[serde(default)]
    pub lifecycle_events: Vec<PlanLifecycleEvent>,
    #[serde(default)]
    pub planner: Option<PlannerRef>,
    #[serde(default)]
    pub planning_state: super::planning_state::PlanningState,
    #[serde(default)]
    pub planning_log: Vec<super::planning_state::PlanningTransition>,
    #[serde(default)]
    pub gates: Vec<super::gates::DecisionGate>,
    #[serde(default)]
    pub planner_requests: Vec<PlannerRequest>,
    #[serde(default)]
    pub planner_inbox: Vec<PlannerInboxEntry>,
    // Plan-level, task-less working points (e.g. skill ledger entries). Append-only.
    #[serde(default)]
    pub strides: Vec<Milestone>,
    // Authorized waivers of pending regression gates. Append-only audit trail.
    #[serde(default)]
    pub regression_gate_overrides: Vec<RegressionGateOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphaned: Option<super::PlanOrphan>,
}

impl Plan {
    pub fn new(title: String, project_path: String, owner: OrchestratorRef) -> Self {
        let id = super::ids::short_id("pl");
        Self {
            id,
            title,
            sections: vec![],
            status: PlanStatus::Active,
            mode: PlanMode::Planning,
            paused_from_mode: None,
            pause_operations: vec![],
            owner,
            team_id: None,
            project_path: project_path.clone(),
            canonical_project_path: project_path,
            created_at: 0,
            updated_at: 0,
            milestone_kinds: vec![],
            loops: vec![],
            elements: vec![],
            tasks: vec![],
            task_groups: vec![],
            tag_facets: vec![],
            links: vec![],
            spec_refs: vec![],
            spec_links: vec![],
            primary_spec_id: None,
            lineage: vec![],
            goal: None,
            heartbeat: OrchestratorHeartbeat::default(),
            control_loop: super::ControlLoop::default(),
            review_policy: ReviewPolicy::default(),
            evidence_reporters: vec![],
            canvas: PlanCanvas::default(),
            owner_history: vec![],
            lifecycle_events: vec![],
            planner: None,
            planning_state: super::planning_state::PlanningState::default(),
            planning_log: vec![],
            gates: vec![],
            planner_requests: vec![],
            planner_inbox: vec![],
            strides: vec![],
            regression_gate_overrides: vec![],
            orphaned: None,
        }
    }

    /// Single source of truth for whether execution (non-planning) tasks may be
    /// dispatched. planning_state is authoritative going forward; an Approved/Execution
    /// lifecycle mode bridges plans predating planning_state. Complete never unlocks.
    pub fn execution_tasks_unlocked(&self) -> bool {
        !matches!(self.mode, PlanMode::Complete | PlanMode::Paused)
            && (self.planning_state.allows_execution_tasks()
                || matches!(self.mode, PlanMode::Approved | PlanMode::Execution))
    }
}

impl Spec {
    pub fn new(title: String, project_path: String) -> Self {
        Self {
            id: super::ids::short_id("sp"),
            plan_id: None,
            title,
            project_path: project_path.clone(),
            canonical_project_path: project_path,
            status: PlanStatus::Active,
            sections: vec![],
            created_at: 0,
            updated_at: 0,
            canvas: PlanCanvas::default(),
        }
    }
}

#[cfg(test)]
mod tests;
