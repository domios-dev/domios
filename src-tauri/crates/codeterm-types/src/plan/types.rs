use super::review::{ReviewAuditEntry, ReviewEvidence, ReviewOverride, ReviewPolicy};
use serde::{Deserialize, Serialize};

pub const INBOX_PLAN_ID: &str = "inbox";
pub const INBOX_PLAN_FILE_STEM: &str = "_inbox";
pub const INBOX_PLAN_TITLE: &str = "Tasks";

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
    pub pane_id: Option<String>,
    pub session_id: Option<String>,
    pub title: Option<String>,
}

impl OrchestratorRef {
    pub fn from_pane(p: &str) -> Self {
        Self {
            pane_id: Some(p.into()),
            ..Default::default()
        }
    }
}

pub type AgentRef = OrchestratorRef;

/// Planner binding on a plan. None = unset; Waived = explicitly solo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannerRef {
    Pane {
        pane_id: String,
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
    pub pane_id: Option<String>,
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
    pub caller_pane_id: Option<String>,
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
    pub delivered_to_pane_id: Option<String>,
    #[serde(default)]
    pub delivered_to_session_id: Option<String>,
    #[serde(default)]
    pub delivered_with_binding_nonce: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskLaneMetadata {
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub workspace_name: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub cleanup_status: Option<String>,
    #[serde(default)]
    pub cleanup_hints: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskCompetitionMetadata {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub attempts: Option<u32>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub judge_hint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskExecutionReportSnapshot {
    pub id: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    /// Pane that submitted a proxy report; None means the executor reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter_pane_id: Option<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub test_cmd: Option<String>,
    #[serde(default)]
    pub test_result: Option<String>,
    #[serde(default)]
    pub skipped: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskExecutionAttempt {
    pub id: String,
    pub executor_pane_id: String,
    #[serde(default)]
    pub executor_title: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub parent_pane_id: Option<String>,
    #[serde(default)]
    pub orchestrator_pane_id: Option<String>,
    #[serde(default)]
    pub mesh_peer: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub report: Option<TaskExecutionReportSnapshot>,
    #[serde(default)]
    pub assigned_at: Option<u64>,
    #[serde(default)]
    pub reported_at: Option<u64>,
    #[serde(default)]
    pub reviewed_at: Option<u64>,
    #[serde(default)]
    pub closed_at: Option<u64>,
    #[serde(default)]
    pub competition_group_id: Option<String>,
    #[serde(default)]
    pub competition_lane: Option<u32>,
    #[serde(default)]
    pub judge_verdict: Option<String>,
    #[serde(default)]
    pub judge_reason: Option<String>,
    #[serde(default)]
    pub judged_at: Option<u64>,
    #[serde(default)]
    pub winner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferRecord {
    pub from: OrchestratorRef,
    pub to: OrchestratorRef,
    pub ts: u64,
    pub reason: String,
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
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    #[serde(default)]
    pub personality: Option<String>,
    #[serde(default)]
    pub personality_provenance: Option<IdentityProvenance>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    pub test_plan: Option<String>,
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
    pub parent_task_id: Option<String>,
    #[serde(default, alias = "dependency_task_ids")]
    pub depends_on: Vec<String>,
    pub assigned_pane_id: Option<String>,
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
    pub review_policy: ReviewPolicy,
    #[serde(default)]
    pub canvas: PlanCanvas,
    #[serde(default)]
    pub owner_history: Vec<TransferRecord>,
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
            review_policy: ReviewPolicy::default(),
            canvas: PlanCanvas::default(),
            owner_history: vec![],
            planner: None,
            planning_state: super::planning_state::PlanningState::default(),
            planning_log: vec![],
            gates: vec![],
            planner_requests: vec![],
            planner_inbox: vec![],
            strides: vec![],
            regression_gate_overrides: vec![],
        }
    }

    /// Single source of truth for whether execution (non-planning) tasks may be
    /// created. planning_state is authoritative going forward; an Approved/Execution
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
mod tests {
    use super::*;
    use crate::plan::review::{ReviewVerdict, DEFAULT_REVIEW_WIP_LIMIT};

    #[test]
    fn plan_roundtrips_and_defaults() {
        let p = Plan::new(
            "Voice UX".into(),
            "/repo".into(),
            OrchestratorRef::from_pane("abc123"),
        );
        assert_eq!(p.status, PlanStatus::Active);
        assert!(p.sections.is_empty() && p.tasks.is_empty() && p.loops.is_empty());
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, p.id);
        assert_eq!(back.title, "Voice UX");
        assert_eq!(back.canonical_project_path, "/repo");
    }

    #[test]
    fn task_groups_and_links_roundtrip() {
        let mut p = Plan::new("T".into(), "/repo".into(), Default::default());
        p.task_groups = vec![TaskGroup {
            key: "phase-1".into(),
            label: "Phase 1".into(),
            color: None,
        }];
        p.links = vec!["pl_other".into()];
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_groups.len(), 1);
        assert_eq!(back.task_groups[0].key, "phase-1");
        assert_eq!(back.links, vec!["pl_other".to_string()]);
    }

    #[test]
    fn old_json_without_new_fields_still_loads() {
        // A plan file written before task_groups / links / Task.group existed.
        let raw = r#"{
            "id": "pl_old",
            "title": "Legacy",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2,
            "tasks": [
                {"id":"task_1","name":"n","type":"implement","model":null,
                 "model_rationale":null,"status":"pending","status_label":null,
                 "section_ids":[],"body":"","intent":null,"assigned_pane_id":null,
                 "peer":null}
            ]
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(p.id, "pl_old");
        assert!(p.canonical_project_path.is_empty());
        assert!(p.task_groups.is_empty());
        assert!(p.tag_facets.is_empty());
        assert!(p.links.is_empty());
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.tasks[0].group, None);
        assert_eq!(p.tasks[0].priority, TaskPriority::P2);
        assert_eq!(p.tasks[0].created_at, 0);
        assert_eq!(p.tasks[0].updated_at, 0);
        assert_eq!(p.tasks[0].status_changed_at, 0);
        assert_eq!(p.tasks[0].handle, None);
        assert!(p.tasks[0].tags.is_empty());
        assert_eq!(p.tasks[0].taken_by_role, None);
        assert_eq!(p.tasks[0].executor_role, None);
        assert_eq!(p.tasks[0].parent_task_id, None);
        assert!(p.tasks[0].depends_on.is_empty());
    }

    #[test]
    fn executor_role_roundtrips() {
        let raw = r#"{
            "id": "pl_exec",
            "title": "Exec",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2,
            "tasks": [
                {"id":"task_1","name":"n","type":"implement","model":null,
                 "model_rationale":null,"status":"pending","status_label":null,
                 "section_ids":[],"body":"","intent":null,"assigned_pane_id":null,
                 "peer":null,"executor_role":"rust-backend-dev"}
            ]
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(
            p.tasks[0].executor_role.as_deref(),
            Some("rust-backend-dev")
        );
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.tasks[0].executor_role.as_deref(),
            Some("rust-backend-dev")
        );
    }

    #[test]
    fn task_status_is_closed_enum() {
        let t: Result<TaskStatus, _> = serde_json::from_str("\"reviewing\"");
        assert!(t.is_err(), "unknown status must reject");
        let ok: TaskStatus = serde_json::from_str("\"in_progress\"").unwrap();
        assert_eq!(ok, TaskStatus::InProgress);
        let ok: TaskStatus = serde_json::from_str("\"in_review\"").unwrap();
        assert_eq!(ok, TaskStatus::InReview);
    }

    #[test]
    fn task_priority_is_closed_enum() {
        let bad: Result<TaskPriority, _> = serde_json::from_str("\"urgent\"");
        assert!(bad.is_err(), "unknown priority must reject");
        let ok: TaskPriority = serde_json::from_str("\"p1\"").unwrap();
        assert_eq!(ok, TaskPriority::P1);
    }

    #[test]
    fn planner_ref_pane_roundtrips() {
        let mut p = Plan::new("P".into(), "/r".into(), Default::default());
        p.planner = Some(PlannerRef::Pane {
            pane_id: "pane_plan".into(),
            session_id: Some("sid".into()),
            title: None,
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"pane\""));
        assert!(json.contains("\"pane_id\":\"pane_plan\""));
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.planner, p.planner);
    }

    #[test]
    fn planner_ref_waived_roundtrips() {
        let mut p = Plan::new("P".into(), "/r".into(), Default::default());
        p.planner = Some(PlannerRef::Waived {
            note: "solo — no review pairing needed".into(),
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"waived\""));
        assert!(json.contains("solo"));
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.planner, p.planner);
    }

    #[test]
    fn planner_ref_defaults_none_on_old_plan() {
        let raw = r#"{
            "id": "pl_old2",
            "title": "Old",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(p.planner, None);
    }

    #[test]
    fn legacy_loopspec_json_loads_without_new_interpretation() {
        let raw = r#"{
            "id":"loop_1",
            "target":"accepted target",
            "budget":3,
            "count":1,
            "stop_when":"oracle holds",
            "status":"running"
        }"#;
        let loop_spec: LoopSpec = serde_json::from_str(raw).unwrap();
        assert_eq!(loop_spec.id, "loop_1");
        assert_eq!(loop_spec.target, "accepted target");
        assert_eq!(loop_spec.budget, 3);
        assert_eq!(loop_spec.count, 1);
        assert_eq!(loop_spec.stop_when.as_deref(), Some("oracle holds"));
        assert_eq!(loop_spec.status, LoopStatus::Running);
        assert_eq!(loop_spec.stagnation, 0);
        assert_eq!(loop_spec.deviation, None);
        assert_eq!(loop_spec.last_iteration_at, None);

        let json = serde_json::to_string(&loop_spec).unwrap();
        let back: LoopSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loop_spec);
    }

    #[test]
    fn planning_state_defaults_drafting_on_old_plan() {
        let raw = r#"{
            "id": "pl_old3",
            "title": "Old",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(
            p.planning_state,
            super::super::planning_state::PlanningState::Drafting
        );
        assert!(p.planning_log.is_empty());
    }

    #[test]
    fn old_plan_without_review_policy_defaults_to_wip_5() {
        let raw = r#"{
            "id": "pl_old4",
            "title": "Old",
            "status": "active",
            "owner": {},
            "project_path": "/repo",
            "created_at": 1,
            "updated_at": 2,
            "tasks": [
                {"id":"task_1","name":"n","type":"implement","model":null,
                 "model_rationale":null,"status":"in_review","status_label":null,
                 "section_ids":[],"body":"","intent":null,"assigned_pane_id":null,
                 "peer":null}
            ]
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(p.review_policy.wip_limit, DEFAULT_REVIEW_WIP_LIMIT);
        assert_eq!(p.review_policy.credit, 0);
        assert_eq!(p.tasks[0].review_round, 0);
        assert_eq!(p.tasks[0].review_oracle, None);
        assert!(p.tasks[0].review_evidence.is_empty());
        assert_eq!(p.tasks[0].review_override, None);
        assert!(p.tasks[0].review_audit.is_empty());
    }

    #[test]
    fn review_evidence_and_policy_roundtrip() {
        let mut p = Plan::new("P".into(), "/r".into(), Default::default());
        p.review_policy = ReviewPolicy {
            wip_limit: 3,
            credit: -2,
            capacity_grants: vec![],
        };
        let mut t = Task {
            id: "task_1".into(),
            name: "n".into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            personality: None,
            personality_provenance: None,
            acceptance_criteria: vec!["ac".into()],
            test_plan: Some("Oracle: run x".into()),
            correctness: None,
            status: TaskStatus::InReview,
            review_stage: ReviewStage::Review,
            qa_state: None,
            review_round: 1,
            review_oracle: Some("sig".into()),
            review_evidence: vec![ReviewEvidence {
                id: "rev_1".into(),
                from: OrchestratorRef::from_pane("qa1"),
                verdict: ReviewVerdict::Bounce,
                note: "needs fix".into(),
                at: 5,
                changed_executable_surfaces: false,
                distinct_oracle: false,
            }],
            review_override: Some(ReviewOverride {
                owner: "andrey".into(),
                reason: "P0 hotfix".into(),
                reproduction: "steps to repro".into(),
                at: 6,
            }),
            review_audit: vec![ReviewAuditEntry {
                id: "aud_1".into(),
                owner: "andrey".into(),
                reason: "P0 hotfix".into(),
                reproduction: "steps to repro".into(),
                round: 2,
                at: 7,
            }],
            status_label: None,
            priority: TaskPriority::P1,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            depends_on: vec![],
            assigned_pane_id: None,
            lane: TaskLaneMetadata::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: ElementCanvas::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            attachments: vec![],
        };
        t.review_round = 1;
        p.tasks.push(t);
        let json = serde_json::to_string(&p).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.review_policy, p.review_policy);
        assert_eq!(back.tasks[0].review_evidence, p.tasks[0].review_evidence);
        assert_eq!(back.tasks[0].review_override, p.tasks[0].review_override);
        assert_eq!(back.tasks[0].review_audit, p.tasks[0].review_audit);
    }

    #[test]
    fn planning_state_roundtrips() {
        use super::super::planning_state::{PlanningAuthor, PlanningState, PlanningTransition};
        let mut p = Plan::new("P".into(), "/r".into(), Default::default());
        p.planning_state = PlanningState::Approved;
        p.planning_log = vec![PlanningTransition {
            from: PlanningState::StructureReady,
            to: PlanningState::Approved,
            by: PlanningAuthor::User,
            note: None,
            at: 9,
        }];
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"planning_state\":\"approved\""));
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.planning_state, PlanningState::Approved);
        assert_eq!(back.planning_log, p.planning_log);
    }
}
