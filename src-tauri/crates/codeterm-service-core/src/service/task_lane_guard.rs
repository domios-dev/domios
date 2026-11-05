//! The one classifier both task-bound entry points ask before a mutating tab
//! exists: the dispatcher and every `agent spawn --plan-task` route. Keeping it
//! here is what makes "same hard stop" literal rather than a second copy that
//! drifts from the dispatcher's.

use super::orchestrator::SpawnAgentParams;
use super::worker_lane::{same_path, verify_worker_lane, LaneProtection};
use super::ServiceContext;
#[cfg(any(test, feature = "test-support"))]
use crate::plan::Task;
use crate::plan::{ExecutionIntent, Plan};
use crate::service::plan::TaskLaneAdmissionRead;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsafeMutatingLane {
    Canonical { canonical: String },
    ForeignLane { worktree: String, owner: String },
}

pub fn canonical_path_of(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    }
}

fn same_checkout(a: &str, b: &str) -> bool {
    codeterm_paths::normalize_cwd(a) == codeterm_paths::normalize_cwd(b)
}

fn foreign_lane_facts<'a>(
    plan: &'a Plan,
    task_id: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    plan.tasks
        .iter()
        .filter(move |task| task.id != task_id && task.handle.as_deref() != Some(task_id))
        .filter_map(|task| {
            task.lane
                .worktree_path
                .as_deref()
                .map(|path| (task.id.as_str(), path))
        })
}

pub fn classify_lane_facts<'a>(
    canonical: &str,
    resolved_worktree: &str,
    lanes: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<UnsafeMutatingLane> {
    if same_checkout(resolved_worktree, canonical) {
        return Some(UnsafeMutatingLane::Canonical {
            canonical: canonical.into(),
        });
    }
    lanes
        .into_iter()
        .find(|(_, path)| same_checkout(path, resolved_worktree))
        .map(|(owner, _)| UnsafeMutatingLane::ForeignLane {
            worktree: resolved_worktree.into(),
            owner: owner.into(),
        })
}

pub fn classify_mutating_lane(
    plan: &Plan,
    task_id: &str,
    resolved_worktree: &str,
) -> Option<UnsafeMutatingLane> {
    classify_lane_facts(
        canonical_path_of(plan),
        resolved_worktree,
        foreign_lane_facts(plan, task_id),
    )
}

/// `(task_id, attempt_id)` of a live attempt holding canonical against integration.
pub fn live_canonical_writer(plan: &Plan) -> Option<(&str, &str)> {
    plan.tasks.iter().find_map(|task| {
        task.execution_attempts
            .iter()
            .find(|attempt| {
                attempt.closed_at.is_none()
                    && attempt.retired_at.is_none()
                    && !attempt.placement.is_isolated_lane()
            })
            .map(|attempt| (task.id.as_str(), attempt.id.as_str()))
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

fn task_bound_mutating_spawn_ref(params: &SpawnAgentParams) -> Option<&str> {
    if params.execution_intent != Some(ExecutionIntent::Mutating) {
        return None;
    }
    if params.task_launch_authority.is_some()
        && crate::service::spawn_plan_task::worker_spawn_plan_task_ref(params).is_none()
    {
        return None;
    }
    task_bound_spawn_ref(params)
}

fn dispatch_remedy(plan_id: &str, task_id: &str) -> String {
    format!(
        "Dispatch the task instead, so the isolated lane is provisioned and recorded before any \
         tab exists: domios plan task dispatch {plan_id} {task_id}"
    )
}

fn canonical_dispatch_remedy(plan_id: &str, task_id: &str, selected_kind: Option<&str>) -> String {
    let kind_flag = selected_kind
        .map(|kind| kind.trim().to_ascii_lowercase())
        .filter(|kind| {
            codeterm_types::agent::role_kind::builtin_kind(kind)
                .is_some_and(|descriptor| descriptor.is_taskless_worker())
        })
        .map(|kind| format!(" --kind {kind}"))
        .unwrap_or_default();
    format!(
        "Use parent-authorized canonical placement so its exclusive attempt is reserved before \
         any tab exists: domios plan task dispatch {plan_id} {task_id} \
         --execution-intent mutating --canonical-writer{kind_flag}"
    )
}

/// Hard stop for a task-bound mutating spawn, decided from values alone so the
/// same verdict is reachable without a live service.
#[cfg(any(test, feature = "test-support"))]
pub fn refuse_unsafe_task_bound_mutating_lane(
    plan: &Plan,
    task: &Task,
    intent: Option<&ExecutionIntent>,
    spawn_cwd: Option<&str>,
) -> Result<(), String> {
    let input = TaskLaneAdmissionRead {
        plan_id: plan.id.clone(),
        task_id: task.id.clone(),
        execution_intent: task.execution_intent,
        canonical_path: canonical_path_of(plan).into(),
        foreign_lanes: foreign_lane_facts(plan, &task.id)
            .map(|(id, path)| crate::service::plan::TaskLaneOwnerRead {
                task_id: id.into(),
                worktree_path: path.into(),
            })
            .collect(),
    };
    refuse_unsafe_task_bound_mutating_lane_with_kind(&input, intent, spawn_cwd, None)
}

fn refuse_unsafe_task_bound_mutating_lane_with_kind(
    input: &TaskLaneAdmissionRead,
    intent: Option<&ExecutionIntent>,
    spawn_cwd: Option<&str>,
    selected_kind: Option<&str>,
) -> Result<(), String> {
    // Read-only verification keeps its existing contract: it owns no lane and
    // is the one shape allowed to run in the canonical checkout.
    if intent != Some(&ExecutionIntent::Mutating) || !input.execution_intent.requires_repository() {
        return Ok(());
    }
    let remedy = dispatch_remedy(&input.plan_id, &input.task_id);
    let task_id = input.task_id.as_str();
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
    match classify_lane_facts(
        &input.canonical_path,
        &provenance.worktree_path,
        input
            .foreign_lanes
            .iter()
            .map(|lane| (lane.task_id.as_str(), lane.worktree_path.as_str())),
    ) {
        None => Ok(()),
        Some(UnsafeMutatingLane::Canonical { canonical }) => Err(format!(
            "spawn refused: a mutating Worker for task '{task_id}' would write in the \
             canonical checkout '{canonical}'. No tab was created. {}",
            canonical_dispatch_remedy(&input.plan_id, task_id, selected_kind)
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
        let Some(task_ref) = task_bound_mutating_spawn_ref(params) else {
            return Ok(());
        };
        let store = &self.plan_store;
        let resolved = store
            .resolve_task_ref(params.plan_id.as_deref(), task_ref)
            .map_err(|error| error.to_string())?;
        let input = store
            .task_lane_admission_projection(&resolved.plan_id, &resolved.task_id)
            .map_err(|error| error.to_string())?;
        let cwd = self.task_bound_spawn_cwd(params);
        refuse_unsafe_task_bound_mutating_lane_with_kind(
            &input,
            params.execution_intent.as_ref(),
            cwd.as_deref(),
            params.role_profile.as_deref(),
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

#[cfg(test)]
mod tests;

/// Whether a resolved checkout is the one the task already recorded.
pub fn recorded_lane_matches(recorded: Option<&str>, resolved: &str) -> bool {
    recorded
        .map(str::trim)
        .filter(|recorded| !recorded.is_empty())
        .is_some_and(|recorded| same_path(recorded, resolved))
}
