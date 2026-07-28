use crate::plan::reconcile::DriftKind;
use crate::plan::{OrchestratorRef, PlanStatus, TaskLaneMetadata, TaskStatus};
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum PlanError {
    NotFound,
    ElementNotFound,
    TaskNotFound,
    OpenSubtasks {
        subtasks: Vec<OpenSubtask>,
    },
    TransitionDenied {
        from: TaskStatus,
        to: TaskStatus,
    },
    UndeclaredKind {
        declared: Vec<String>,
    },
    BudgetExhausted {
        count: u32,
        budget: u32,
    },
    BadStatus,
    BadMode,
    BadPriority,
    TaskCreationRequiresApproval,
    HeartbeatRequiresExecution,
    TaskPauseRequiresReason,
    PauseRequiresReason,
    InProgressRequiresTab,
    BlockedRequiresLabel,
    QaNoteRequired,
    QaSlotUnavailable,
    QaNotRunning,
    OverrideReasonRequired,
    OverrideNotAuthorized,
    ReviewWipExceeded {
        wip_count: usize,
        wip_limit: u32,
        credit: i64,
    },
    ReviewEvidenceRequired,
    ReviewRoundExceeded {
        round: u32,
        max_rounds: u32,
    },
    ReviewOracleScopeExpansion,
    ReviewOverrideIncomplete,
    ReviewNotInReview,
    GateNotFound,
    GateAlreadyAnswered,
    GateRequiresQuestion,
    GroupNotFound,
    WaitingFocusRequiresOpenGate,
    DependencyTaskNotFound {
        task_id: String,
    },
    DependencyTaskReferenced {
        task_id: String,
        dependent_task_ids: Vec<String>,
    },
    DependencySelfReference,
    DependencyCycle,
    WritePending {
        operation: String,
        committed: bool,
        committed_id: Option<String>,
        message: String,
    },
    ReadbackMismatch {
        operation: String,
        message: String,
    },
    TeamAlreadyHasActivePlan {
        team_id: String,
        existing_plan_id: String,
        existing_title: String,
    },
    PlanAlreadyBoundToTeam {
        plan_id: String,
        team_id: String,
    },
    ActivePlanRequiresTeam {
        plan_id: String,
    },
    PlanActivationRequiresUnarchive {
        plan_id: String,
    },
    PlanSkillAlreadyExists {
        slug: String,
    },
    InvalidPlanSkillSlug,
    Io(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenSubtask {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub handle: Option<String>,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::NotFound => write!(f, "plan not found"),
            PlanError::ElementNotFound => write!(f, "element not found"),
            PlanError::TaskNotFound => write!(f, "task not found"),
            PlanError::OpenSubtasks { subtasks } => {
                let listed = subtasks
                    .iter()
                    .map(|task| {
                        let name = if task.name.is_empty() {
                            task.id.as_str()
                        } else {
                            task.name.as_str()
                        };
                        format!("{name} ({})", task.id)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "task has open subtasks: {listed}")
            }
            PlanError::TransitionDenied { from, to } => {
                write!(
                    f,
                    "task status transition {from:?} -> {to:?} is not allowed"
                )
            }
            PlanError::UndeclaredKind { declared } => {
                write!(
                    f,
                    "undeclared milestone kind; declared: {}",
                    declared.join(", ")
                )
            }
            PlanError::BudgetExhausted { count, budget } => {
                write!(f, "loop budget {count}/{budget} reached")
            }
            PlanError::BadStatus => write!(f, "invalid status"),
            PlanError::BadMode => write!(f, "invalid plan mode"),
            PlanError::BadPriority => write!(f, "invalid priority"),
            PlanError::TaskCreationRequiresApproval => write!(
                f,
                "plan is still in planning mode; run `codeterm plan approve <plan>` before creating tasks, or pass --force for deliberate draft tasks"
            ),
            PlanError::HeartbeatRequiresExecution => write!(
                f,
                "heartbeat is for active execution; approve the plan or store planning-only policy in spec/plan sections or mem"
            ),
            PlanError::InProgressRequiresTab => write!(
                f,
                "in_progress requires an executor tab; set both in one command: codeterm plan task update <plan> <task> --status in_progress --tab <tab-id> (CODETERM_STATE_INVARIANTS=off for emergencies)"
            ),
            PlanError::BlockedRequiresLabel => write!(
                f,
                "blocked is a claim, not a holding state: pass --status-label \"<what unblocks it>\" or open a decision gate"
            ),
            PlanError::TaskPauseRequiresReason => write!(
                f,
                "pausing a task requires a reason; run `codeterm plan task pause <plan> <task> --reason \"<why>\"`"
            ),
            PlanError::PauseRequiresReason => write!(
                f,
                "pausing a plan requires a reason; run `codeterm plan pause <plan> --reason \"<why>\"`"
            ),
            PlanError::QaNoteRequired => write!(
                f,
                "QA transitions require evidence; pass --note \"<what changed or what was tested>\""
            ),
            PlanError::QaSlotUnavailable => write!(f, "QA queue has no available running slot"),
            PlanError::QaNotRunning => write!(f, "task is not running in QA; claim it first"),
            PlanError::OverrideReasonRequired => write!(
                f,
                "overriding a regression gate requires an explicit reason; pass --reason \"<why the frontier may advance past the gate>\""
            ),
            PlanError::OverrideNotAuthorized => write!(
                f,
                "only the plan Orchestrator (owner) may override a regression gate"
            ),
            PlanError::ReviewWipExceeded {
                wip_count,
                wip_limit,
                credit,
            } => write!(
                f,
                "review WIP exceeded: {wip_count}/{wip_limit} in_review (credit={credit}); close a review before dispatching more implementation work"
            ),
            PlanError::ReviewEvidenceRequired => write!(
                f,
                "in_review requires evidence: pass an executable exact QA plan (test_plan + acceptance_criteria) or attach independent review evidence first"
            ),
            PlanError::ReviewRoundExceeded { round, max_rounds } => write!(
                f,
                "review round {round} exceeds the default max of {max_rounds}; record a P0 exception first with `codeterm plan review override <plan> <task> --owner <o> --reason <r> --reproduction <repro>`"
            ),
            PlanError::ReviewOracleScopeExpansion => write!(
                f,
                "correction review cannot expand the fixed acceptance oracle without new reproducible release-impact evidence; record a P0 override to proceed"
            ),
            PlanError::ReviewOverrideIncomplete => write!(
                f,
                "review override requires non-empty owner, reason, and reproduction"
            ),
            PlanError::ReviewNotInReview => write!(f, "task is not in_review"),
            PlanError::GateNotFound => write!(f, "decision gate not found"),
            PlanError::GateAlreadyAnswered => {
                write!(f, "gate is already answered; answers are immutable")
            }
            PlanError::GateRequiresQuestion => write!(f, "gate requires a non-empty question"),
            PlanError::GroupNotFound => write!(
                f,
                "task group not found; declare it first with `codeterm plan groups set <plan> --groups <key:label,...>`"
            ),
            PlanError::WaitingFocusRequiresOpenGate => write!(
                f,
                "waiting-focus requires naming an open gate id; use `codeterm plan decision list <plan>`"
            ),
            PlanError::DependencyTaskNotFound { task_id } => {
                write!(f, "dependency task '{task_id}' does not exist")
            }
            PlanError::DependencyTaskReferenced {
                task_id,
                dependent_task_ids,
            } => {
                write!(
                    f,
                    "task '{task_id}' is still referenced by dependent tasks: {}",
                    dependent_task_ids.join(", ")
                )
            }
            PlanError::DependencySelfReference => {
                write!(f, "task cannot depend on itself")
            }
            PlanError::DependencyCycle => {
                write!(f, "dependency graph would contain a cycle")
            }
            PlanError::WritePending {
                operation,
                committed,
                committed_id,
                message,
            } => {
                let id = committed_id.as_deref().unwrap_or("unknown");
                write!(
                    f,
                    "{operation} pending after write (committed={committed}, id={id}): {message}"
                )
            }
            PlanError::ReadbackMismatch { operation, message } => {
                write!(f, "{operation} readback mismatch: {message}")
            }
            PlanError::TeamAlreadyHasActivePlan {
                team_id,
                existing_plan_id,
                existing_title,
            } => write!(
                f,
                "team {team_id} already owns active plan {existing_plan_id} «{existing_title}»; a team binds 1:1 to a plan. Create or use another Team for this effort (spawn a new team-bound orchestrator), or archive/complete {existing_plan_id} first."
            ),
            PlanError::PlanAlreadyBoundToTeam { plan_id, team_id } => write!(
                f,
                "plan {plan_id} is already bound to team {team_id}; a plan binds 1:1 to a team"
            ),
            PlanError::ActivePlanRequiresTeam { plan_id } => write!(
                f,
                "active plan {plan_id} requires a live Team; adopt it with `codeterm team adopt-plan <team-id> {plan_id}`"
            ),
            PlanError::PlanActivationRequiresUnarchive { plan_id } => write!(
                f,
                "plan {plan_id} is archived; restore it with `codeterm plan unarchive {plan_id}` so its live Team binding is validated"
            ),
            PlanError::PlanSkillAlreadyExists { slug } => {
                write!(f, "plan skill '{slug}' already exists")
            }
            PlanError::InvalidPlanSkillSlug => {
                write!(f, "plan skill slug is empty after normalization")
            }
            PlanError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl PlanError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            PlanError::NotFound => serde_json::json!({ "type": "not_found" }),
            PlanError::ElementNotFound => serde_json::json!({ "type": "element_not_found" }),
            PlanError::TaskNotFound => serde_json::json!({ "type": "task_not_found" }),
            PlanError::OpenSubtasks { subtasks } => {
                serde_json::json!({ "type": "open_subtasks", "subtasks": subtasks })
            }
            PlanError::TransitionDenied { from, to } => {
                serde_json::json!({ "type": "transition_denied", "from": from, "to": to })
            }
            PlanError::UndeclaredKind { declared } => {
                serde_json::json!({ "type": "undeclared_kind", "declared": declared })
            }
            PlanError::BudgetExhausted { count, budget } => {
                serde_json::json!({ "type": "budget_exhausted", "count": count, "budget": budget })
            }
            PlanError::BadStatus => serde_json::json!({ "type": "bad_status" }),
            PlanError::BadMode => serde_json::json!({ "type": "bad_mode" }),
            PlanError::BadPriority => serde_json::json!({ "type": "bad_priority" }),
            PlanError::TaskCreationRequiresApproval => {
                serde_json::json!({ "type": "task_creation_requires_approval" })
            }
            PlanError::HeartbeatRequiresExecution => {
                serde_json::json!({ "type": "heartbeat_requires_execution" })
            }
            PlanError::InProgressRequiresTab => {
                serde_json::json!({ "type": "in_progress_requires_tab" })
            }
            PlanError::BlockedRequiresLabel => {
                serde_json::json!({ "type": "blocked_requires_label" })
            }
            PlanError::TaskPauseRequiresReason => {
                serde_json::json!({ "type": "task_pause_requires_reason" })
            }
            PlanError::PauseRequiresReason => {
                serde_json::json!({ "type": "pause_requires_reason" })
            }
            PlanError::QaNoteRequired => serde_json::json!({ "type": "qa_note_required" }),
            PlanError::QaSlotUnavailable => serde_json::json!({ "type": "qa_slot_unavailable" }),
            PlanError::QaNotRunning => serde_json::json!({ "type": "qa_not_running" }),
            PlanError::OverrideReasonRequired => {
                serde_json::json!({ "type": "override_reason_required" })
            }
            PlanError::OverrideNotAuthorized => {
                serde_json::json!({ "type": "override_not_authorized" })
            }
            PlanError::ReviewWipExceeded {
                wip_count,
                wip_limit,
                credit,
            } => serde_json::json!({
                "type": "review_wip_exceeded",
                "wip_count": wip_count,
                "wip_limit": wip_limit,
                "credit": credit
            }),
            PlanError::ReviewEvidenceRequired => {
                serde_json::json!({ "type": "review_evidence_required" })
            }
            PlanError::ReviewRoundExceeded { round, max_rounds } => serde_json::json!({
                "type": "review_round_exceeded",
                "round": round,
                "max_rounds": max_rounds
            }),
            PlanError::ReviewOracleScopeExpansion => {
                serde_json::json!({ "type": "review_oracle_scope_expansion" })
            }
            PlanError::ReviewOverrideIncomplete => {
                serde_json::json!({ "type": "review_override_incomplete" })
            }
            PlanError::ReviewNotInReview => serde_json::json!({ "type": "review_not_in_review" }),
            PlanError::GateNotFound => serde_json::json!({ "type": "gate_not_found" }),
            PlanError::GateAlreadyAnswered => {
                serde_json::json!({ "type": "gate_already_answered" })
            }
            PlanError::GateRequiresQuestion => {
                serde_json::json!({ "type": "gate_requires_question" })
            }
            PlanError::GroupNotFound => serde_json::json!({ "type": "group_not_found" }),
            PlanError::WaitingFocusRequiresOpenGate => {
                serde_json::json!({ "type": "waiting_focus_requires_open_gate" })
            }
            PlanError::DependencyTaskNotFound { task_id } => {
                serde_json::json!({ "type": "dependency_task_not_found", "task_id": task_id })
            }
            PlanError::DependencyTaskReferenced {
                task_id,
                dependent_task_ids,
            } => {
                serde_json::json!({
                    "type": "dependency_task_referenced",
                    "task_id": task_id,
                    "dependent_task_ids": dependent_task_ids
                })
            }
            PlanError::DependencySelfReference => {
                serde_json::json!({ "type": "dependency_self_reference" })
            }
            PlanError::DependencyCycle => {
                serde_json::json!({ "type": "dependency_cycle" })
            }
            PlanError::WritePending {
                operation,
                committed,
                committed_id,
                message,
            } => {
                serde_json::json!({
                    "type": "write_pending",
                    "operation": operation,
                    "committed": committed,
                    "committed_id": committed_id,
                    "message": message
                })
            }
            PlanError::ReadbackMismatch { operation, message } => {
                serde_json::json!({
                    "type": "readback_mismatch",
                    "operation": operation,
                    "message": message
                })
            }
            PlanError::TeamAlreadyHasActivePlan {
                team_id,
                existing_plan_id,
                existing_title,
            } => serde_json::json!({
                "type": "team_already_has_active_plan",
                "team_id": team_id,
                "existing_plan_id": existing_plan_id,
                "existing_title": existing_title
            }),
            PlanError::PlanAlreadyBoundToTeam { plan_id, team_id } => serde_json::json!({
                "type": "plan_already_bound_to_team",
                "plan_id": plan_id,
                "team_id": team_id
            }),
            PlanError::ActivePlanRequiresTeam { plan_id } => serde_json::json!({
                "type": "active_plan_requires_team",
                "plan_id": plan_id
            }),
            PlanError::PlanActivationRequiresUnarchive { plan_id } => serde_json::json!({
                "type": "plan_activation_requires_unarchive",
                "plan_id": plan_id
            }),
            PlanError::PlanSkillAlreadyExists { slug } => serde_json::json!({
                "type": "plan_skill_already_exists",
                "slug": slug
            }),
            PlanError::InvalidPlanSkillSlug => {
                serde_json::json!({ "type": "invalid_plan_skill_slug" })
            }
            PlanError::Io(message) => serde_json::json!({ "type": "io", "message": message }),
        }
    }
}

#[derive(Default)]
pub struct NewTask {
    pub name: String,
    pub kind: String,
    pub section_ids: Vec<String>,
    pub model: Option<String>,
    pub model_rationale: Option<String>,
    pub personality: Option<String>,
    pub personality_provenance: Option<crate::plan::IdentityProvenance>,
    pub acceptance_criteria: Vec<String>,
    pub test_plan: Option<String>,
    pub qa_plan: Option<String>,
    pub correctness: Option<String>,
    pub body: String,
    pub role_guidance: Option<String>,
    pub executor_role: Option<String>,
    pub competition: Option<crate::plan::TaskCompetitionMetadata>,
    pub intent: Option<String>,
    pub parent_task_id: Option<String>,
    pub depends_on: Vec<String>,
    pub assigned_tab_id: Option<String>,
    pub lane: TaskLaneMetadata,
    pub group: Option<String>,
    pub priority: Option<crate::plan::TaskPriority>,
    pub handle: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Default, Clone)]
pub struct TaskLaneUpdate {
    pub tab_id: Option<Option<String>>,
    pub workspace_id: Option<Option<String>>,
    pub workspace_name: Option<Option<String>>,
    pub worktree_path: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub base: Option<Option<String>>,
    pub destination: Option<Option<String>>,
    pub cleanup_status: Option<Option<String>>,
    pub cleanup_hints: Option<Vec<String>>,
}

impl TaskLaneUpdate {
    pub fn is_empty(&self) -> bool {
        self.tab_id.is_none()
            && self.workspace_id.is_none()
            && self.workspace_name.is_none()
            && self.worktree_path.is_none()
            && self.branch.is_none()
            && self.base.is_none()
            && self.destination.is_none()
            && self.cleanup_status.is_none()
            && self.cleanup_hints.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexEntry {
    pub plan_id: String,
    pub file_path: PathBuf,
    pub canonical_project_path: String,
    pub owner: OrchestratorRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub title: String,
    pub status: PlanStatus,
    pub updated_at: u64,
    pub assigned_tab_ids: Vec<String>,
    pub task_count: usize,
    pub open_task_count: usize,
    pub section_count: usize,
    pub element_count: usize,
    pub loop_count: usize,
    pub has_goal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecIndexEntry {
    pub spec_id: String,
    pub file_path: PathBuf,
    pub title: String,
    pub status: PlanStatus,
    pub updated_at: u64,
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReconcileFixResult {
    pub plan_id: String,
    pub task_id: String,
    pub kind: DriftKind,
    pub action: String,
}
