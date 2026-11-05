use crate::plan::{ExecutionIntent, PlanMode, ProviderAllowance, Task, TaskStatus};

#[derive(Clone, Debug)]
pub struct DispatchUndo {
    pub(super) plan_id: String,
    pub(super) task_id: String,
    pub(super) attempt_id: String,
    pub(super) before_mode: PlanMode,
    pub(super) after_mode: PlanMode,
    before: StartupFields,
    respawn: bool,
    after_owner_epoch: u64,
    after_destination: Option<String>,
    attempts: Vec<AttemptUndo>,
}

#[derive(Clone, Debug)]
pub struct DispatchStart {
    pub attempt_id: String,
    pub undo: DispatchUndo,
}

#[derive(Clone, Debug)]
struct StartupFields {
    status: TaskStatus,
    status_label: Option<String>,
    status_changed_at: u64,
    assigned_tab_id: Option<String>,
    taken_by_role: Option<String>,
    peer: Option<String>,
    lane_tab: Option<String>,
    destination: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    execution_intent: ExecutionIntent,
    allowance: Option<ProviderAllowance>,
    owner_epoch: u64,
    owner_tab_id: Option<String>,
    owner_session_id: Option<String>,
}

#[derive(Clone, Debug)]
struct AttemptUndo {
    id: String,
    before_closed: Option<u64>,
    after_closed: Option<u64>,
    before_retired: Option<u64>,
    after_retired: Option<u64>,
}

fn revert<T: Clone + PartialEq>(current: &mut T, before: &T, after: &T) {
    if before != after && current == after {
        *current = before.clone();
    }
}

impl StartupFields {
    fn capture(task: &Task) -> Self {
        Self {
            status: task.status.clone(),
            status_label: task.status_label.clone(),
            status_changed_at: task.status_changed_at,
            assigned_tab_id: task.assigned_tab_id.clone(),
            taken_by_role: task.taken_by_role.clone(),
            peer: task.peer.clone(),
            lane_tab: task.lane.tab_id.clone(),
            destination: task.lane.destination.clone(),
            workspace_id: task.lane.workspace_id.clone(),
            workspace_name: task.lane.workspace_name.clone(),
            worktree_path: task.lane.worktree_path.clone(),
            branch: task.lane.branch.clone(),
            base: task.lane.base.clone(),
            execution_intent: task.interruption.execution_intent.clone(),
            allowance: task.interruption.allowance.clone(),
            owner_epoch: task.interruption.owner_epoch,
            owner_tab_id: task.interruption.owner_tab_id.clone(),
            owner_session_id: task.interruption.owner_session_id.clone(),
        }
    }

    fn restore(&self, task: &mut Task, respawn: bool) {
        macro_rules! field {
            ($name:ident, $target:expr) => {
                *$target = self.$name.clone();
            };
        }
        if respawn || self.status == TaskStatus::Blocked {
            field!(status, &mut task.status);
            field!(status_changed_at, &mut task.status_changed_at);
        }
        field!(status_label, &mut task.status_label);
        field!(assigned_tab_id, &mut task.assigned_tab_id);
        field!(taken_by_role, &mut task.taken_by_role);
        field!(peer, &mut task.peer);
        field!(lane_tab, &mut task.lane.tab_id);
        if !respawn {
            field!(workspace_id, &mut task.lane.workspace_id);
            field!(workspace_name, &mut task.lane.workspace_name);
            field!(worktree_path, &mut task.lane.worktree_path);
            field!(branch, &mut task.lane.branch);
            field!(base, &mut task.lane.base);
            field!(execution_intent, &mut task.interruption.execution_intent);
            field!(allowance, &mut task.interruption.allowance);
            field!(owner_epoch, &mut task.interruption.owner_epoch);
            field!(owner_tab_id, &mut task.interruption.owner_tab_id);
            field!(owner_session_id, &mut task.interruption.owner_session_id);
        }
    }
}

impl DispatchUndo {
    pub(in crate::service_plan) fn capture(
        plan_id: &str,
        mode: &PlanMode,
        task: &Task,
        attempt_id: &str,
        respawn_source: Option<&str>,
    ) -> Self {
        let fields = StartupFields::capture(task);
        Self {
            plan_id: plan_id.into(),
            task_id: task.id.clone(),
            attempt_id: attempt_id.into(),
            before_mode: mode.clone(),
            after_mode: mode.clone(),
            before: fields,
            respawn: respawn_source.is_some(),
            after_owner_epoch: task.interruption.owner_epoch,
            after_destination: task.lane.destination.clone(),
            attempts: task
                .execution_attempts
                .iter()
                .filter(|attempt| {
                    (attempt.closed_at.is_none() && attempt.competition_group_id.is_none())
                        || respawn_source == Some(attempt.id.as_str())
                })
                .map(|attempt| AttemptUndo {
                    id: attempt.id.clone(),
                    before_closed: attempt.closed_at,
                    after_closed: attempt.closed_at,
                    before_retired: attempt.retired_at,
                    after_retired: attempt.retired_at,
                })
                .collect(),
        }
    }

    pub(in crate::service_plan) fn finish(&mut self, mode: &PlanMode, task: &Task) {
        self.after_mode = mode.clone();
        self.after_owner_epoch = task.interruption.owner_epoch;
        self.after_destination = task.lane.destination.clone();
        for undo in &mut self.attempts {
            if let Some(attempt) = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == undo.id)
            {
                undo.after_closed = attempt.closed_at;
                undo.after_retired = attempt.retired_at;
            }
        }
    }

    pub(super) fn row_attempt_ids(&self) -> Vec<String> {
        std::iter::once(self.attempt_id.clone())
            .chain(self.attempts.iter().map(|attempt| attempt.id.clone()))
            .collect()
    }

    pub(super) fn apply(&self, task: &mut Task) {
        self.before.restore(task, self.respawn);
        revert(
            &mut task.lane.destination,
            &self.before.destination,
            &self.after_destination,
        );
        for undo in &self.attempts {
            if let Some(attempt) = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == undo.id)
            {
                revert(
                    &mut attempt.closed_at,
                    &undo.before_closed,
                    &undo.after_closed,
                );
                revert(
                    &mut attempt.retired_at,
                    &undo.before_retired,
                    &undo.after_retired,
                );
            }
        }
    }

    pub(super) fn owner_epoch_current(&self, task: &Task) -> bool {
        task.interruption.owner_epoch == self.after_owner_epoch
            || (self.after_owner_epoch == 0 && task.interruption.owner_epoch == 1)
    }

    pub(super) fn restores_blocked(&self) -> bool {
        self.before.status == TaskStatus::Blocked
    }
}

impl crate::service_plan::PlanStore {
    pub fn begin_task_dispatch_with_undo(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        title: &str,
        lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<DispatchStart, crate::service_plan::PlanError> {
        self.begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
            plan_id,
            task_id,
            tab_id,
            title,
            lane,
            ExecutionIntent::Unspecified,
            None,
            None,
            &crate::plan::ids::short_id("attempt"),
            None,
            crate::plan::WorkerPlacement::IsolatedLane,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            now_ms,
        )
    }
}
