//! The explicit Orchestrator opt-in that places one mutating Worker in the
//! canonical checkout. Exclusivity reuses the checkout occupancy preflight.

use serde_json::{json, Value};

use crate::plan::{ExecutionIntent, Plan, Task, WorkerPlacement};
use crate::service::{live_canonical_writer, verify_worker_lane, LaneProtection};

use super::super::JsonRpcError;
use super::lane_resolution::{checkout_required, invalid_dispatch};

pub(crate) const PLACEMENT_CANONICAL_WRITER: &str = "canonical_writer";
const PLACEMENT_ISOLATED_LANE: &str = "isolated_lane";
pub(crate) const REFUSAL_LANE_INTEGRATION_WHILE_CANONICAL_WRITER: &str =
    "lane.integration_while_canonical_writer_holds_checkout";
pub(crate) const CANONICAL_WRITER_HOLDS_CHECKOUT_CODE: i32 = -32015;

pub(crate) const CANONICAL_WRITER_BRIEF: &str = "## Placement: canonical writer
The Orchestrator dispatched you with an explicit canonical-writer opt-in. For this attempt you are the only writer in the canonical integration checkout; there is no isolated lane.
- Edit and commit directly on the checked-out integration branch. Never switch branches, merge, rebase, reset, or push.
- Execute only the exact commands listed under `## Authorized gates`, through the repository wrapper. Broad, release and integration gates stay with the Orchestrator; list anything else under GATES REQUESTED.
- Commit everything before reporting `in_review`: a dirty tree refuses the report and is a bounce. Pass your final commit as `--sha` and list every commit id you made under CHANGES.
- The Orchestrator accepts or bounces your work. Lane integration into canonical waits until that verdict.

";

pub(super) fn dispatch_worker_placement(
    p: Option<&Value>,
    intent: &ExecutionIntent,
    task: &Task,
) -> Result<WorkerPlacement, JsonRpcError> {
    match p.and_then(|params| params.get("placement")) {
        None | Some(Value::Null) => return Ok(WorkerPlacement::IsolatedLane),
        Some(value) if value.as_str() == Some(PLACEMENT_ISOLATED_LANE) => {
            return Ok(WorkerPlacement::IsolatedLane)
        }
        Some(value) if value.as_str() == Some(PLACEMENT_CANONICAL_WRITER) => {}
        Some(_) => {
            return Err(invalid_dispatch(format!(
                "placement must be {PLACEMENT_ISOLATED_LANE} or {PLACEMENT_CANONICAL_WRITER}"
            )))
        }
    }
    if *intent != ExecutionIntent::Mutating {
        return Err(invalid_dispatch(format!(
            "placement {PLACEMENT_CANONICAL_WRITER} requires execution_intent mutating"
        )));
    }
    if !task.execution_intent.requires_repository() {
        return Err(invalid_dispatch(format!(
            "placement {PLACEMENT_CANONICAL_WRITER} requires a repository task; task '{}' is \
             non_repository",
            task.id
        )));
    }
    if let Some(lane) = task
        .lane
        .worktree_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return Err(invalid_dispatch(format!(
            "task '{}' records the isolated lane '{lane}'; a canonical writer never abandons \
             recorded lane work. No Worker was spawned.",
            task.id
        )));
    }
    Ok(WorkerPlacement::CanonicalWriter)
}

/// A canonical writer commits on the integration branch, so a detached HEAD is refused.
pub(super) fn verify_canonical_writer_checkout(canonical_cwd: &str) -> Result<(), JsonRpcError> {
    verify_worker_lane(
        canonical_cwd,
        canonical_cwd,
        None,
        None,
        LaneProtection::Enforced,
    )
    .map(|_| ())
    .map_err(checkout_required)
}

pub(super) fn compose_dispatch_brief(brief: String, placement: WorkerPlacement) -> String {
    match placement {
        WorkerPlacement::IsolatedLane => brief,
        WorkerPlacement::CanonicalWriter => format!("{brief}{CANONICAL_WRITER_BRIEF}"),
    }
}

/// Accepting a lane task authorizes merging it into canonical, which the live
/// canonical writer holds exclusively until its own verdict.
pub(crate) fn refuse_lane_integration_while_canonical_held(
    plan: &Plan,
    task_id: &str,
) -> Result<(), JsonRpcError> {
    let Some(task) = plan.tasks.iter().find(|task| task.id == task_id) else {
        return Ok(());
    };
    let records_lane = task
        .lane
        .worktree_path
        .as_deref()
        .is_some_and(|path| !path.trim().is_empty());
    if !records_lane {
        return Ok(());
    }
    let Some((holder_task_id, holder_attempt_id)) = live_canonical_writer(plan) else {
        return Ok(());
    };
    if holder_task_id == task.id {
        return Ok(());
    }
    let reason = format!(
        "lane integration refused: live attempt '{holder_attempt_id}' of task \
         '{holder_task_id}' holds the canonical checkout as its exclusive writer, so lane task \
         '{task_id}' cannot be accepted into it yet."
    );
    let safe_alternative = format!(
        "record the canonical writer's verdict first (codeterm plan review accept|bounce {} \
         {holder_task_id} --note \"<evidence>\"), then accept this lane.",
        plan.id
    );
    Err(JsonRpcError {
        code: CANONICAL_WRITER_HOLDS_CHECKOUT_CODE,
        message: format!("{reason} Safe alternative: {safe_alternative}"),
        data: Some(json!({
            "type": "canonical_writer_holds_checkout",
            "stage": "lane_integration",
            "violation_id": REFUSAL_LANE_INTEGRATION_WHILE_CANONICAL_WRITER,
            "reason": reason,
            "holder_task_id": holder_task_id,
            "holder_attempt_id": holder_attempt_id,
            "safe_alternative": safe_alternative,
        })),
    })
}
