//! The one classifier both task-bound entry points ask before a mutating tab
//! exists: the dispatcher and every `agent spawn --plan-task` route. Keeping it
//! here is what makes "same hard stop" literal rather than a second copy that
//! drifts from the dispatcher's.

use super::orchestrator::SpawnAgentParams;
use super::worker_lane::{same_path, verify_worker_lane, LaneProtection};
use super::ServiceContext;
use crate::plan::{ExecutionIntent, Plan, Task};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnsafeMutatingLane {
    Canonical { canonical: String },
    ForeignLane { worktree: String, owner: String },
}

pub(crate) fn canonical_path_of(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    }
}

fn same_checkout(a: &str, b: &str) -> bool {
    codeterm_paths::normalize_cwd(a) == codeterm_paths::normalize_cwd(b)
}

fn foreign_lane_owner<'a>(plan: &'a Plan, task_id: &str, worktree: &str) -> Option<&'a str> {
    plan.tasks
        .iter()
        .filter(|task| task.id != task_id && task.handle.as_deref() != Some(task_id))
        .find(|task| {
            task.lane
                .worktree_path
                .as_deref()
                .is_some_and(|recorded| same_checkout(recorded, worktree))
        })
        .map(|task| task.id.as_str())
}

/// `None` means the checkout is this task's own isolated lane.
pub(crate) fn classify_mutating_lane(
    plan: &Plan,
    task_id: &str,
    resolved_worktree: &str,
) -> Option<UnsafeMutatingLane> {
    let canonical = canonical_path_of(plan);
    if same_checkout(resolved_worktree, canonical) {
        return Some(UnsafeMutatingLane::Canonical {
            canonical: canonical.to_string(),
        });
    }
    foreign_lane_owner(plan, task_id, resolved_worktree).map(|owner| {
        UnsafeMutatingLane::ForeignLane {
            worktree: resolved_worktree.to_string(),
            owner: owner.to_string(),
        }
    })
}

/// The task a spawn binds, by either flag that can bind one.
fn task_bound_spawn_ref(params: &SpawnAgentParams) -> Option<&str> {
    fn named(value: &Option<String>) -> Option<&str> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
    named(&params.plan_task).or_else(|| named(&params.task_id))
}

fn dispatch_remedy(plan_id: &str, task_id: &str) -> String {
    format!(
        "Dispatch the task instead, so the isolated lane is provisioned and recorded before any \
         tab exists: codeterm plan task dispatch {plan_id} {task_id}"
    )
}

/// Hard stop for a task-bound mutating spawn, decided from values alone so the
/// same verdict is reachable without a live service.
pub(crate) fn refuse_unsafe_task_bound_mutating_lane(
    plan: &Plan,
    task: &Task,
    intent: Option<&ExecutionIntent>,
    spawn_cwd: Option<&str>,
) -> Result<(), String> {
    // Read-only verification keeps its existing contract: it owns no lane and
    // is the one shape allowed to run in the canonical checkout.
    if intent != Some(&ExecutionIntent::Mutating) || !task.execution_intent.requires_repository() {
        return Ok(());
    }
    let remedy = dispatch_remedy(&plan.id, &task.id);
    let task_id = task.id.as_str();
    let Some(cwd) = spawn_cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return Err(format!(
            "spawn refused: a mutating Worker for task '{task_id}' named no checkout, so it \
             would inherit an unverified working directory. No tab was created. {remedy}"
        ));
    };
    let provenance =
        verify_worker_lane(cwd, cwd, None, None, LaneProtection::Enforced).map_err(|error| {
            format!(
                "spawn refused: a mutating Worker for task '{task_id}' would write in \
                 '{cwd}', which is not a verified isolated lane ({error}). No tab was \
                 created. {remedy}"
            )
        })?;
    match classify_mutating_lane(plan, task_id, &provenance.worktree_path) {
        None => Ok(()),
        Some(UnsafeMutatingLane::Canonical { canonical }) => Err(format!(
            "spawn refused: a mutating Worker for task '{task_id}' would write in the \
             canonical checkout '{canonical}'. No tab was created. {remedy}"
        )),
        Some(UnsafeMutatingLane::ForeignLane { worktree, owner }) => Err(format!(
            "spawn refused: a mutating Worker for task '{task_id}' would write in \
             '{worktree}', the lane recorded for task '{owner}'. No tab was created. {remedy}"
        )),
    }
}

impl ServiceContext {
    /// The route-level wrapper: resolve which task and which checkout, then ask
    /// the shared verdict. Runs before a tab exists on every spawn route.
    pub(crate) fn refuse_unsafe_task_bound_mutating_spawn(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
        if params.execution_intent != Some(ExecutionIntent::Mutating) {
            return Ok(());
        }
        let Some(task_ref) = task_bound_spawn_ref(params) else {
            return Ok(());
        };
        let store = &self.plan_store;
        let resolved = store
            .resolve_task_ref(params.plan_id.as_deref(), task_ref)
            .map_err(|error| error.to_string())?;
        let plan = store
            .get(&resolved.plan_id)
            .ok_or_else(|| "plan not found".to_string())?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == resolved.task_id)
            .ok_or_else(|| "task not found".to_string())?;
        let cwd = self.task_bound_spawn_cwd(params);
        refuse_unsafe_task_bound_mutating_lane(
            &plan,
            task,
            params.execution_intent.as_ref(),
            cwd.as_deref(),
        )
    }

    /// A spawn that names no checkout inherits the daemon's working directory,
    /// which is never a verified lane — so `None` is refused, not guessed at.
    fn task_bound_spawn_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        params
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|cwd| !cwd.is_empty())
            .map(str::to_string)
            .or_else(|| self.worker_spawn_workspace_cwd(params))
    }
}

/// Whether a resolved checkout is the one the task already recorded.
pub(crate) fn recorded_lane_matches(recorded: Option<&str>, resolved: &str) -> bool {
    recorded
        .map(str::trim)
        .filter(|recorded| !recorded.is_empty())
        .is_some_and(|recorded| same_path(recorded, resolved))
}
