use crate::plan::{TaskExecutionAttempt, TaskExecutorTermination};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ExecutorSourceRead {
    pub id: String,
    pub executor_tab_id: String,
    pub provider: Option<String>,
    pub session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub mesh_peer: Option<String>,
}

#[derive(Clone, Copy)]
pub struct ExecutorSourceFields<'a> {
    pub id: &'a str,
    pub executor_tab_id: &'a str,
    pub provider: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub session_binding_nonce: Option<&'a str>,
    pub mesh_peer: Option<&'a str>,
}

impl<'a> From<&'a ExecutorSourceRead> for ExecutorSourceFields<'a> {
    fn from(source: &'a ExecutorSourceRead) -> Self {
        Self {
            id: &source.id,
            executor_tab_id: &source.executor_tab_id,
            provider: source.provider.as_deref(),
            session_id: source.session_id.as_deref(),
            session_binding_nonce: source.session_binding_nonce.as_deref(),
            mesh_peer: source.mesh_peer.as_deref(),
        }
    }
}

impl<'a> From<&'a TaskExecutionAttempt> for ExecutorSourceFields<'a> {
    fn from(source: &'a TaskExecutionAttempt) -> Self {
        Self {
            id: &source.id,
            executor_tab_id: &source.executor_tab_id,
            provider: source.provider.as_deref(),
            session_id: source.session_id.as_deref(),
            session_binding_nonce: source.session_binding_nonce.as_deref(),
            mesh_peer: source.mesh_peer.as_deref(),
        }
    }
}

fn same_executor(left: &TaskExecutionAttempt, right: ExecutorSourceFields<'_>) -> bool {
    !right.id.trim().is_empty()
        && !right.executor_tab_id.trim().is_empty()
        && left.id == right.id
        && left.executor_tab_id == right.executor_tab_id
        && left.session_id.as_deref() == right.session_id
        && left.session_binding_nonce.as_deref() == right.session_binding_nonce
        && left.mesh_peer.is_none()
        && right.mesh_peer.is_none()
}

impl PlanStore {
    pub fn record_task_executor_termination<'a>(
        &self,
        plan: &str,
        task: &str,
        source: impl Into<ExecutorSourceFields<'a>>,
        now: u64,
    ) -> Result<(), PlanError> {
        let source = source.into();
        self.with_task_rows(
            plan,
            task,
            super::super::store_db::hot_write::TaskHistory::Attempts,
            now,
            |edit| {
                let attempt = edit
                    .task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| same_executor(attempt, source))
                    .ok_or_else(|| PlanError::Io("executor termination identity changed".into()))?;
                let already_confirmed = attempt.executor_termination.as_ref().is_some_and(
                    |receipt| {
                        receipt.attempt_id == source.id
                            && receipt.executor_tab_id == source.executor_tab_id
                            && receipt.session_id.as_deref() == source.session_id
                            && receipt.session_binding_nonce.as_deref()
                                == source.session_binding_nonce
                    },
                );
                if !already_confirmed {
                    attempt.executor_termination = Some(TaskExecutorTermination {
                        attempt_id: source.id.into(),
                        executor_tab_id: source.executor_tab_id.into(),
                        session_id: source.session_id.map(str::to_owned),
                        session_binding_nonce: source.session_binding_nonce.map(str::to_owned),
                        confirmed_at: now,
                    });
                }
                if edit.task.status == crate::plan::TaskStatus::Cancelled {
                    let retiring = attempt.closed_at.is_none() || attempt.retired_at.is_none();
                    attempt.closed_at.get_or_insert(now);
                    attempt.retired_at.get_or_insert(now);
                    let mut owner_released = false;
                    if edit.task.assigned_tab_id.as_deref() == Some(source.executor_tab_id) {
                        edit.task.assigned_tab_id = None;
                        owner_released = true;
                    }
                    if edit.task.lane.tab_id.as_deref() == Some(source.executor_tab_id) {
                        edit.task.lane.tab_id = None;
                        owner_released = true;
                    }
                    if edit.task.interruption.owner_tab_id.as_deref()
                        == Some(source.executor_tab_id)
                        && edit.task.interruption.owner_session_id.as_deref() == source.session_id
                    {
                        edit.task.interruption.owner_tab_id = None;
                        edit.task.interruption.owner_session_id = None;
                        edit.task.interruption.handoff_required = false;
                        owner_released = true;
                    }
                    if retiring || owner_released {
                        edit.task.updated_at = now;
                    }
                }
                Ok(())
            },
        )
    }

    pub fn task_executor_termination_confirmed<'a>(
        &self,
        plan: &str,
        task: &str,
        source: impl Into<ExecutorSourceFields<'a>>,
    ) -> Result<bool, PlanError> {
        self.database
            .executor_termination_confirmed(plan, task, source.into())
    }
}
