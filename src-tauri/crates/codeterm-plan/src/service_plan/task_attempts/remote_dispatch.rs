use crate::plan::{
    RemoteDispatchState, TaskExecutionAttempt, TaskExecutionReportSnapshot, TaskStatus,
};
use crate::service_plan::PlanError;

use super::super::PlanStore;

impl PlanStore {
    pub fn begin_remote_dispatch_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        peer_name: &str,
        provider: &str,
        base: &str,
        destination: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let peer_id = peer_id.trim().to_string();
        let peer_name = peer_name.trim().to_string();
        if peer_id.is_empty() || peer_name.is_empty() {
            return Err(PlanError::Io(
                "remote dispatch peer identity is incomplete".into(),
            ));
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::LiveAndTarget(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
                    return Err(PlanError::Io("remote dispatch task is terminal".into()));
                }
                if task
                    .execution_attempts
                    .iter()
                    .any(|attempt| attempt.closed_at.is_none())
                {
                    return Err(PlanError::Io(
                        "task already has a live execution attempt".into(),
                    ));
                }
                let executor_tab_id = format!("mesh-worker:{attempt_id}");
                task.execution_attempts.push(TaskExecutionAttempt {
                    id: attempt_id.to_string(),
                    executor_tab_id: executor_tab_id.clone(),
                    executor_title: Some(format!("Worker@{peer_name}")),
                    provider: Some(provider.to_string()),
                    agent_type: Some(provider.to_string()),
                    role: Some("Worker".into()),
                    mesh_peer: Some(peer_id.clone()),
                    remote_dispatch_state: Some(RemoteDispatchState::PeerPreflight),
                    base: Some(base.to_string()),
                    destination: Some(destination.to_string()),
                    assigned_at: Some(now_ms),
                    ..Default::default()
                });
                task.lane.destination = Some(destination.to_string());
                task.assigned_tab_id = Some(executor_tab_id);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    pub fn set_remote_dispatch_state(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        state: RemoteDispatchState,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let peer_id = peer_id.trim();
        if peer_id.is_empty() {
            return Err(PlanError::Io(
                "remote dispatch peer identity is empty".into(),
            ));
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let worker_id = format!("mesh-worker:{attempt_id}");
                let clear_assignment = {
                    let attempt = task
                        .execution_attempts
                        .iter_mut()
                        .find(|attempt| attempt.id == attempt_id)
                        .ok_or(PlanError::TaskNotFound)?;
                    if attempt
                        .mesh_peer
                        .as_deref()
                        .is_some_and(|recorded| recorded != peer_id)
                    {
                        return Err(PlanError::Io(
                            "remote dispatch peer identity does not match the attempt".into(),
                        ));
                    }
                    attempt.mesh_peer = Some(peer_id.to_string());
                    attempt.remote_dispatch_state = Some(state);
                    let terminal = matches!(
                        state,
                        RemoteDispatchState::PeerMissingRepo
                            | RemoteDispatchState::PeerMissingBase
                            | RemoteDispatchState::PeerOffline
                            | RemoteDispatchState::PeerRefused
                            | RemoteDispatchState::RelayLost
                            | RemoteDispatchState::RemoteAckTimeout
                    );
                    if terminal {
                        attempt.closed_at.get_or_insert(now_ms);
                        attempt.closure_reason =
                            Some(format!("remote_dispatch_{state:?}").to_ascii_lowercase());
                    }
                    if terminal && attempt.executor_tab_id == worker_id {
                        attempt.executor_tab_id.clear();
                        true
                    } else {
                        false
                    }
                };
                if clear_assignment && task.assigned_tab_id.as_deref() == Some(worker_id.as_str()) {
                    task.assigned_tab_id = None;
                }
                Ok(())
            },
        )
    }

    pub fn acknowledge_remote_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        worker_tab_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if attempt.closed_at.is_some()
                    || attempt.retired_at.is_some()
                    || attempt.mesh_peer.as_deref() != Some(peer_id)
                    || !matches!(
                        attempt.remote_dispatch_state,
                        Some(
                            RemoteDispatchState::AwaitingWorkerAck
                                | RemoteDispatchState::Acknowledged
                        )
                    )
                    || attempt.executor_tab_id != worker_tab_id
                {
                    return Err(PlanError::Io(
                        "remote worker ACK does not match an origin attempt".into(),
                    ));
                }
                if attempt.acknowledged_at.is_some() {
                    return Ok(false);
                }
                attempt.acknowledged_at = Some(now_ms);
                attempt.remote_dispatch_state = Some(RemoteDispatchState::Acknowledged);
                task.status = crate::plan::TaskStatus::InProgress;
                task.status_changed_at = now_ms;
                task.status_label = Some("acknowledged".into());
                task.updated_at = now_ms;
                Ok(true)
            },
        )
    }

    pub fn mark_remote_ack_timeout(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        worker_tab_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let Some(attempt) = task.execution_attempts.iter_mut().find(|attempt| {
                    attempt.id == attempt_id
                        && attempt.mesh_peer.as_deref() == Some(peer_id)
                        && attempt.executor_tab_id == worker_tab_id
                        && attempt.remote_dispatch_state
                            == Some(RemoteDispatchState::AwaitingWorkerAck)
                        && attempt.acknowledged_at.is_none()
                        && attempt.closed_at.is_none()
                }) else {
                    return Ok(false);
                };
                attempt.remote_dispatch_state = Some(RemoteDispatchState::RemoteAckTimeout);
                attempt.closed_at = Some(now_ms);
                attempt.retired_at = Some(now_ms);
                attempt.closure_reason = Some("remote_ack_timeout".into());
                if task.assigned_tab_id.as_deref() == Some(worker_tab_id) {
                    task.assigned_tab_id = None;
                }
                if task.status == TaskStatus::InProgress {
                    task.status = TaskStatus::Pending;
                    task.status_changed_at = now_ms;
                }
                task.status_label = Some("remote_ack_timeout".into());
                task.updated_at = now_ms;
                Ok(true)
            },
        )
        .or_else(|error| match error {
            PlanError::TaskNotFound => Ok(false),
            error => Err(error),
        })
    }

    pub fn mark_remote_peer_offline_before_ack(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        worker_tab_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let Some(attempt) = task.execution_attempts.iter_mut().find(|attempt| {
                    attempt.id == attempt_id
                        && attempt.mesh_peer.as_deref() == Some(peer_id)
                        && attempt.executor_tab_id == worker_tab_id
                        && attempt.remote_dispatch_state
                            == Some(RemoteDispatchState::AwaitingWorkerAck)
                        && attempt.acknowledged_at.is_none()
                        && attempt.closed_at.is_none()
                }) else {
                    return Ok(false);
                };
                attempt.remote_dispatch_state = Some(RemoteDispatchState::PeerOffline);
                attempt.closed_at = Some(now_ms);
                attempt.retired_at = Some(now_ms);
                attempt.closure_reason = Some("remote_peer_offline_before_ack".into());
                if task.assigned_tab_id.as_deref() == Some(worker_tab_id) {
                    task.assigned_tab_id = None;
                }
                if task.status == TaskStatus::InProgress {
                    task.status = TaskStatus::Pending;
                    task.status_changed_at = now_ms;
                }
                task.status_label = Some("peer_offline".into());
                task.updated_at = now_ms;
                Ok(true)
            },
        )
        .or_else(|error| match error {
            PlanError::TaskNotFound => Ok(false),
            error => Err(error),
        })
    }

    pub fn record_remote_dispatch_lane(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        worker_tab_id: &str,
        branch: &str,
        worktree_path: &str,
        base: &str,
        provider: &str,
        session_binding_nonce: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if session_binding_nonce.trim().is_empty() {
            return Err(PlanError::Io("remote worker binding nonce is empty".into()));
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if attempt.closed_at.is_some()
                    || attempt.retired_at.is_some()
                    || attempt.mesh_peer.as_deref() != Some(peer_id)
                    || attempt.remote_dispatch_state != Some(RemoteDispatchState::PeerPreflight)
                {
                    return Err(PlanError::Io(
                        "remote lane does not match a pending origin attempt".into(),
                    ));
                }
                attempt.executor_tab_id = worker_tab_id.to_string();
                attempt.session_binding_nonce = Some(session_binding_nonce.to_string());
                attempt.provider = Some(provider.to_string());
                attempt.branch = Some(branch.to_string());
                attempt.remote_worktree_path = Some(worktree_path.to_string());
                attempt.base = Some(base.to_string());
                attempt.remote_dispatch_state = Some(RemoteDispatchState::AwaitingWorkerAck);
                task.lane.branch = Some(branch.to_string());
                task.lane.worktree_path = None;
                task.lane.base = Some(base.to_string());
                task.assigned_tab_id = Some(worker_tab_id.to_string());
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    pub fn ingest_remote_dispatch_report(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        report: TaskExecutionReportSnapshot,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if attempt.mesh_peer.as_deref() != Some(peer_id)
                    || report.reporter_tab_id.as_deref() != Some(&attempt.executor_tab_id)
                {
                    return Err(PlanError::Io(
                        "remote report does not match an origin attempt".into(),
                    ));
                }
                if attempt
                    .report
                    .as_ref()
                    .is_some_and(|existing| existing.id == report.id)
                {
                    return Ok(false);
                }
                if attempt.closed_at.is_some()
                    || attempt.retired_at.is_some()
                    || attempt.remote_dispatch_state != Some(RemoteDispatchState::Acknowledged)
                {
                    return Err(PlanError::Io(
                        "remote report does not match an acknowledged origin attempt".into(),
                    ));
                }
                if task.status.is_terminal() {
                    return Ok(false);
                }
                attempt.report = Some(report);
                attempt.reported_at = Some(now_ms);
                attempt.remote_dispatch_state = Some(RemoteDispatchState::Reported);
                if task.status != TaskStatus::InReview {
                    task.status = TaskStatus::InReview;
                    task.status_changed_at = now_ms;
                }
                task.status_label = None;
                task.updated_at = now_ms;
                Ok(true)
            },
        )
    }

    pub fn mark_remote_dispatch_integrated(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        lane_tip: &str,
        merge_commit: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if attempt.remote_dispatch_state == Some(RemoteDispatchState::Integrated) {
                    if attempt.remote_lane_tip.as_deref() != Some(lane_tip)
                        || attempt.remote_merge_commit.as_deref() != Some(merge_commit)
                    {
                        return Err(PlanError::Io(
                            "remote lane integration result differs from the recorded merge".into(),
                        ));
                    }
                    return Ok(());
                }
                if attempt.judge_verdict.as_deref() != Some("accepted")
                    || !matches!(
                        attempt.remote_dispatch_state,
                        Some(
                            RemoteDispatchState::Reported
                                | RemoteDispatchState::PeerOffline
                                | RemoteDispatchState::RelayLost
                        )
                    )
                    || attempt.branch.is_none()
                {
                    return Err(PlanError::Io(
                        "remote lane integration requires an origin-accepted report".into(),
                    ));
                }
                attempt.remote_lane_tip = Some(lane_tip.to_string());
                attempt.remote_merge_commit = Some(merge_commit.to_string());
                attempt.remote_dispatch_state = Some(RemoteDispatchState::Integrated);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}

#[cfg(test)]
#[path = "remote_dispatch/tests.rs"]
mod tests;
