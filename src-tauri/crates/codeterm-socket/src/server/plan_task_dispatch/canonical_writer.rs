//! The explicit Orchestrator opt-in that places one mutating Worker in the
//! canonical checkout. Exclusivity reuses the checkout occupancy preflight.

use serde_json::{json, Value};

use codeterm_service_core::plan::{ExecutionIntent, WorkerPlacement};
use codeterm_service_core::service::plan::LaneIntegrationRead;
use codeterm_service_core::service::{verify_worker_lane, LaneProtection};

use super::super::JsonRpcError;
use super::lane_resolution::{checkout_required, invalid_dispatch};
use codeterm_service_core::service::plan::TaskDispatchPreflightRead;

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
    task: &TaskDispatchPreflightRead,
) -> Result<WorkerPlacement, JsonRpcError> {
    let default_placement = if *intent == ExecutionIntent::ReadOnly {
        WorkerPlacement::ReadOnly
    } else if !task.execution_intent.requires_repository() {
        WorkerPlacement::NonRepository
    } else {
        WorkerPlacement::IsolatedLane
    };
    match p.and_then(|params| params.get("placement")) {
        None | Some(Value::Null) => return Ok(default_placement),
        Some(value) if value.as_str() == Some(PLACEMENT_ISOLATED_LANE) => {
            return Ok(default_placement)
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

pub(super) fn resolve_placement(
    p: Option<&Value>,
    intent: &ExecutionIntent,
    task: &TaskDispatchPreflightRead,
    plans: &[codeterm_service_core::service::plan::shared_checkout::CheckoutPeerRead],
) -> Result<(WorkerPlacement, String), JsonRpcError> {
    let requested = p
        .and_then(|params| params.get("placement"))
        .and_then(Value::as_str);
    if p.and_then(|params| params.get("placement"))
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err(invalid_dispatch(
            "placement must be shared, lane or canonical_writer",
        ));
    }
    if *intent == ExecutionIntent::Mutating && task.execution_intent.requires_repository() {
        if let Some(cwd) = p
            .and_then(|params| params.get("cwd"))
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.trim().is_empty())
        {
            let checkout = verify_worker_lane(cwd, cwd, None, None, LaneProtection::Enforced)
                .map_err(checkout_required)?;
            let canonical = if task.canonical_project_path.trim().is_empty() {
                &task.project_path
            } else {
                &task.canonical_project_path
            };
            if !codeterm_service_core::service::same_path(&checkout.worktree_path, canonical) {
                return match requested {
                    None | Some("lane" | PLACEMENT_ISOLATED_LANE) => Ok((
                        WorkerPlacement::IsolatedLane,
                        "explicit checkout outside the shared integration checkout".into(),
                    )),
                    Some("shared") => Err(invalid_dispatch(
                        "shared placement requires the integration checkout; use lane placement for an explicit foreign checkout",
                    )),
                    Some(PLACEMENT_CANONICAL_WRITER) => Err(invalid_dispatch(
                        "canonical writer placement conflicts with the explicit checkout; no Worker was spawned",
                    )),
                    Some(value) => Err(invalid_dispatch(format!("unknown placement '{value}'"))),
                };
            }
        }
    }
    if *intent == ExecutionIntent::Mutating
        && task.execution_intent.requires_repository()
        && requested != Some(PLACEMENT_CANONICAL_WRITER)
    {
        return codeterm_service_core::service::plan::shared_checkout::choose_placement_facts(
            plans,
            &task.plan_id,
            &task.id,
            if task.canonical_project_path.trim().is_empty() {
                &task.project_path
            } else {
                &task.canonical_project_path
            },
            &task.files,
            task.lane.worktree_path.is_some() || task.lane.branch.is_some(),
            task.build_breaking,
            requested,
        )
        .map_err(invalid_dispatch);
    }
    dispatch_worker_placement(p, intent, task).map(|placement| {
        (
            placement,
            match placement {
                WorkerPlacement::ReadOnly => "read-only attempt requires no checkout",
                WorkerPlacement::NonRepository => "non-repository task requires no checkout",
                _ => "explicit canonical writer",
            }
            .into(),
        )
    })
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
        WorkerPlacement::IsolatedLane => format!("{brief}## Placement: isolated lane\nWork in the assigned lane. `domios plan task extend-files` is for shared Workers only; lane Workers do not use it. If the Orchestrator asks you to continue in canonical after integrating the lane, report your literal canonical task commit SHA; its diff must touch only declared task paths.\n\n"),
        WorkerPlacement::ReadOnly | WorkerPlacement::NonRepository => brief,
        WorkerPlacement::CanonicalWriter => format!("{brief}{CANONICAL_WRITER_BRIEF}"),
        WorkerPlacement::Shared => format!("{brief}## Placement: shared checkout\nUse only your task's declared edit paths. Request additional paths with `domios plan task extend-files --edit-file PATH`. Commit only owned paths with `git commit -- <paths>` in one task commit; report that literal SHA. Never change branches, merge, reset or sweep another Worker's changes. Keep the checkout compiling; report compiler failures in foreign paths to the Orchestrator.\n\n"),
    }
}

/// Accepting a lane task authorizes merging it into canonical, which the live
/// canonical writer holds exclusively until its own verdict.
pub(crate) fn refuse_lane_integration_while_canonical_held(
    input: Option<&LaneIntegrationRead>,
) -> Result<(), JsonRpcError> {
    let Some(input) = input else {
        return Ok(());
    };
    let task_id = &input.task_id;
    let holder_task_id = &input.holder.task_id;
    let holder_attempt_id = &input.holder.attempt_id;
    let canonical = &input.canonical_path;
    if codeterm_service_core::service::worker_lane::git_value(
        &input.worktree_path,
        ["rev-parse", "--verify", "HEAD^{commit}"],
    )
    .ok()
    .is_some_and(|tip| {
        codeterm_service_core::service::worker_lane::is_ancestor(canonical, &tip, "HEAD")
    }) {
        return Ok(());
    }
    let reason = format!(
        "lane integration refused: live attempt '{holder_attempt_id}' of task \
         '{holder_task_id}' holds the canonical checkout for source work, so lane task \
         '{task_id}' cannot be accepted into it yet."
    );
    let safe_alternative = format!(
        "record the canonical writer's verdict first (domios plan review accept|bounce {} \
         {holder_task_id} --note \"<evidence>\"), then accept this lane.",
        input.plan_id
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
