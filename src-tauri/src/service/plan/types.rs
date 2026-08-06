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
    GateNotFound,
    GateAlreadyAnswered,
    GateRequiresQuestion,
    GroupNotFound,
    WaitingFocusRequiresOpenGate,
    DependencyTaskNotFound {
        task_id: String,
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
                "rejected: nothing changed. in_progress needs an executor tab set in the same command. use `codeterm plan task dispatch <plan> <task>` (binds tab+status atomically), or `codeterm plan task update <plan> <task> --status in_progress --tab <tab-id>`. CODETERM_STATE_INVARIANTS=off is an emergency escape hatch and was NOT applied here"
            ),
            PlanError::BlockedRequiresLabel => write!(
                f,
                "blocked is a claim, not a holding state: pass --status-label \"<what unblocks it>\" or open a decision gate"
            ),
            PlanError::TaskPauseRequiresReason => write!(
                f,
                "pausing a task requires a reason; run `codeterm plan task pause <plan> <task> --reason \"<why>\"`"
            ),
            PlanError::PauseRequiresReason => write!(f, "pausing a plan requires a reason"),
            PlanError::QaNoteRequired => write!(
                f,
                "QA transitions require evidence; pass --note \"<what changed or what was tested>\""
            ),
            PlanError::QaSlotUnavailable => write!(f, "QA queue has no available running slot"),
            PlanError::QaNotRunning => write!(f, "task is not running in QA; claim it first"),
            PlanError::GateNotFound => write!(f, "decision gate not found"),
            PlanError::GateAlreadyAnswered => write!(f, "gate is already answered; answers are immutable"),
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
    pub owner: OrchestratorRef,
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
