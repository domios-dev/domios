use codeterm_types::plan::{
    ControlLoop, DecisionGate, EffortLink, Goal, LoopSpec, MilestoneKind, OrchestratorRef,
    PlanCanvas, PlanEvidenceReporterGrant, PlanLifecycleEvent, PlanMode, PlanOrphan,
    PlanPauseOperation, PlanStatus, PlannerInboxEntry, PlannerRef, PlannerRequest, PlanningState,
    PlanningTransition, RegressionGateOverride, ReviewPolicy, SpecLink, TagFacet, TaskGroup,
    TransferRecord,
};

#[derive(serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct PlanDocumentHeader {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub mode: PlanMode,
    #[serde(default)]
    pub paused_from_mode: Option<PlanMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pause_operations: Vec<PlanPauseOperation>,
    pub owner: OrchestratorRef,
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
    pub control_loop: ControlLoop,
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
    pub planning_state: PlanningState,
    #[serde(default)]
    pub planning_log: Vec<PlanningTransition>,
    #[serde(default)]
    pub gates: Vec<DecisionGate>,
    #[serde(default)]
    pub planner_requests: Vec<PlannerRequest>,
    #[serde(default)]
    pub planner_inbox: Vec<PlannerInboxEntry>,
    #[serde(default)]
    pub regression_gate_overrides: Vec<RegressionGateOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphaned: Option<PlanOrphan>,
}
