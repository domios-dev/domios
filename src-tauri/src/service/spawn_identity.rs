//! Rename-proof assignment identity and roleless-spawn attribution helpers.
//! Pure functions, factored out of `spawn.rs` to keep it under the size gate.

use super::{AgentSpawnSpec, SpawnTabSpec};
use crate::multiplexer::types::TaskBinding;

pub(crate) fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The tab id to attribute a roleless spawn to: the CLI caller if known, else
/// an explicit parent. This is the pane that will own the audit relationship.
pub(crate) fn roleless_spawner(agent: &AgentSpawnSpec, spec: &SpawnTabSpec) -> Option<String> {
    agent
        .spawned_by_tab_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| spec.parent_tab_id.clone())
        .or_else(|| spec.creator_tab_id.clone())
}

/// A stable, one-line digest of the `--task` handle for ad-hoc lanes.
pub(crate) fn task_spawn_handle(task: Option<&str>) -> Option<String> {
    let raw = task?.lines().find(|l| !l.trim().is_empty())?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.chars().take(120).collect())
}

/// Build the rename-proof identity from the plan/task env and `--task` handle.
/// `None` when a spawn carries no assignment at all (e.g. a bare shell agent).
pub(crate) fn task_binding_from_spawn(agent: &AgentSpawnSpec) -> Option<TaskBinding> {
    let plan_id = agent
        .extra_env
        .get("CODETERM_PLAN_ID")
        .filter(|s| !s.trim().is_empty())
        .cloned();
    let task_id = agent
        .extra_env
        .get("CODETERM_TASK_ID")
        .filter(|s| !s.trim().is_empty())
        .cloned();
    let spawn_handle = task_spawn_handle(agent.task.as_deref());
    if plan_id.is_none() && task_id.is_none() && spawn_handle.is_none() {
        return None;
    }
    Some(TaskBinding {
        plan_id,
        task_id,
        spawn_handle,
        bound_at: now_unix_secs(),
    })
}
