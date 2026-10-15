use crate::plan::{ExecutionIntent, Task, TaskExecutionIntent};
use crate::service::{verify_worker_lane, LaneProtection, WorkerLaneProvenance};

use super::task_attempts::TaskRespawnDraft;

fn respawn_requires_git_lane(
    execution_intent: &ExecutionIntent,
    task_intent: TaskExecutionIntent,
    task_worktree: Option<&str>,
    attempt_worktree: Option<&str>,
) -> bool {
    task_worktree.is_some()
        || attempt_worktree.is_some()
        || (*execution_intent != ExecutionIntent::ReadOnly && task_intent.requires_repository())
}

pub fn verify_respawn_lane(
    canonical_cwd: &str,
    task: &Task,
    draft: &TaskRespawnDraft,
) -> Result<Option<WorkerLaneProvenance>, String> {
    if draft.task_id != task.id
        || !task
            .execution_attempts
            .iter()
            .any(|attempt| attempt.id == draft.source_attempt.id)
    {
        return Err("respawn source attempt does not belong to the task".into());
    }
    if !respawn_requires_git_lane(
        &task.interruption.execution_intent,
        task.execution_intent,
        task.lane.worktree_path.as_deref(),
        draft.source_attempt.worktree_path.as_deref(),
    ) {
        if !std::path::Path::new(&draft.cwd).is_dir() {
            return Err(format!(
                "respawn cwd is not an existing directory: {}",
                draft.cwd
            ));
        }
        return Ok(None);
    }
    verify_worker_lane(
        canonical_cwd,
        &draft.cwd,
        task.lane.branch.as_deref(),
        task.lane.base.as_deref(),
        LaneProtection::for_dispatch_intent(&task.interruption.execution_intent),
    )
    .map(Some)
}

#[cfg(test)]
mod tests;
