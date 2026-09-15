use crate::plan::{PlanMode, Task};
use crate::service::plan::{DispatchRollback, PlanStore};
use crate::service::{same_path, ProvisionedWorkerLane};

use super::rollback_failed_dispatch;

/// Roll back one dispatch attempt through the plan's current-attempt fence. A
/// superseded attempt leaves the task, and any tab or worktree the newer attempt holds, untouched.
#[allow(clippy::too_many_arguments)]
pub(super) fn roll_back_dispatch_attempt<C>(
    store: &PlanStore,
    plan_id: &str,
    original_task: &Task,
    original_plan_mode: &PlanMode,
    tab_id: &str,
    attempt_id: &str,
    lane: Option<ProvisionedWorkerLane>,
    close_tab: C,
    now_ms: u64,
) -> Result<(), String>
where
    C: FnOnce() -> Result<(), String>,
{
    let rollback = store
        .roll_back_task_dispatch_attempt(
            plan_id,
            original_task,
            original_plan_mode.clone(),
            tab_id,
            attempt_id,
            now_ms,
        )
        .map_err(|error| error.to_string());
    let (lane, owns_tab) = match &rollback {
        Ok(DispatchRollback::Superseded {
            live_tab_id,
            live_worktree_path,
        }) => (
            lane.filter(|lane| {
                live_worktree_path
                    .as_deref()
                    .is_none_or(|live| !same_path(live, &lane.cwd))
            }),
            live_tab_id.as_deref() != Some(tab_id),
        ),
        _ => (lane, true),
    };
    rollback_failed_dispatch(
        lane,
        || rollback.map(|_| ()),
        || if owns_tab { close_tab() } else { Ok(()) },
    )
}
