use crate::plan::{
    PlanError, RemoteDispatchState, TaskExecutionAttempt, TaskExecutionReportSnapshot, TaskStatus,
};

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
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let peer_id = peer_id.trim().to_string();
        let peer_name = peer_name.trim().to_string();
        if peer_id.is_empty() || peer_name.is_empty() {
            return Err(PlanError::Io("remote dispatch peer identity is incomplete".into()));
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
                return Err(PlanError::Io("remote dispatch task is terminal".into()));
            }
            if task.execution_attempts.iter().any(|attempt| attempt.closed_at.is_none()) {
                return Err(PlanError::Io("task already has a live execution attempt".into()));
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
                assigned_at: Some(now_ms),
                ..Default::default()
            });
            task.assigned_tab_id = Some(executor_tab_id);
            task.updated_at = now_ms;
            Ok(())
        })
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
            return Err(PlanError::Io("remote dispatch peer identity is empty".into()));
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.mesh_peer.as_deref().is_some_and(|recorded| recorded != peer_id) {
                return Err(PlanError::Io(
                    "remote dispatch peer identity does not match the attempt".into(),
                ));
            }
            attempt.mesh_peer = Some(peer_id.to_string());
            attempt.remote_dispatch_state = Some(state);
            if matches!(state, RemoteDispatchState::PeerMissingRepo | RemoteDispatchState::PeerMissingBase) {
                attempt.closed_at.get_or_insert(now_ms);
                attempt.closure_reason = Some(format!("remote_dispatch_{state:?}".to_ascii_lowercase()));
            }
            Ok(())
        })
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
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.mesh_peer.as_deref() != Some(peer_id)
                || !matches!(
                    attempt.remote_dispatch_state,
                    Some(RemoteDispatchState::AwaitingWorkerAck | RemoteDispatchState::Acknowledged)
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
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.mesh_peer.as_deref() != Some(peer_id)
                || attempt.remote_dispatch_state != Some(RemoteDispatchState::PeerPreflight)
            {
                return Err(PlanError::Io("remote lane does not match a pending origin attempt".into()));
            }
            attempt.executor_tab_id = worker_tab_id.to_string();
            attempt.provider = Some(provider.to_string());
            attempt.branch = Some(branch.to_string());
            attempt.worktree_path = Some(worktree_path.to_string());
            attempt.base = Some(base.to_string());
            attempt.remote_dispatch_state = Some(RemoteDispatchState::AwaitingWorkerAck);
            task.lane.branch = Some(branch.to_string());
            task.lane.worktree_path = Some(worktree_path.to_string());
            task.lane.base = Some(base.to_string());
            task.assigned_tab_id = Some(worker_tab_id.to_string());
            task.updated_at = now_ms;
            Ok(())
        })
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
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.mesh_peer.as_deref() != Some(peer_id)
                || attempt.remote_dispatch_state != Some(RemoteDispatchState::Acknowledged)
                || report.reporter_tab_id.as_deref() != Some(&attempt.executor_tab_id)
            {
                return Err(PlanError::Io("remote report does not match an origin attempt".into()));
            }
            if attempt.report.as_ref().is_some_and(|existing| existing.id == report.id) {
                return Ok(false);
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt};
    use crate::service_plan::NewTask;

    fn remote_attempt() -> (tempfile::TempDir, PlanStore, String, String, String) {
        let root = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(root.path());
        let mut plan = Plan::new(
            "Remote dispatch".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("origin-orchestrator"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Remote task".into(),
                    kind: "implement".into(),
                    acceptance_criteria: vec!["report reaches origin".into()],
                    test_plan: Some("focused test".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let attempt_id = "attempt_remote".to_string();
        store
            .with_plan(&plan.id, 3, |plan| {
                let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
                task.execution_attempts.push(TaskExecutionAttempt {
                    id: attempt_id.clone(),
                    executor_tab_id: "mesh-worker:attempt_remote".into(),
                    mesh_peer: Some("peer-origin".into()),
                    remote_dispatch_state: Some(RemoteDispatchState::PeerPreflight),
                    ..Default::default()
                });
                Ok(())
            })
            .unwrap();
        (root, store, plan.id, task_id, attempt_id)
    }

    #[test]
    fn remote_dispatch_ack_is_authenticated_and_idempotent() {
        let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
        assert!(store
            .acknowledge_remote_dispatch(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                "mesh-worker:attempt_remote",
                4,
            )
            .unwrap());
        assert!(!store
            .acknowledge_remote_dispatch(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                "mesh-worker:attempt_remote",
                5,
            )
            .unwrap());
        let detail = store.show_task(&plan_id, &task_id).unwrap();
        let attempt = &detail.task.execution_attempts[0];
        assert_eq!(attempt.remote_dispatch_state, Some(RemoteDispatchState::Acknowledged));
        assert_eq!(attempt.acknowledged_at, Some(4));
    }

    #[test]
    fn remote_dispatch_peer_mismatch_cannot_change_attempt_state() {
        let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
        assert!(store
            .acknowledge_remote_dispatch(
                &plan_id,
                &task_id,
                &attempt_id,
                "spoofed-peer",
                "mesh-worker:attempt_remote",
                4,
            )
            .is_err());
        let detail = store.show_task(&plan_id, &task_id).unwrap();
        let attempt = &detail.task.execution_attempts[0];
        assert_eq!(attempt.remote_dispatch_state, Some(RemoteDispatchState::PeerPreflight));
        assert_eq!(attempt.acknowledged_at, None);
    }

    #[test]
    fn mesh_dispatch_records_origin_attempt_and_relays_ack_and_report() {
        let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
        store
            .record_remote_dispatch_lane(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                "remote-tab-1",
                "lane/remote-attempt",
                "D:/peer/repo-worktrees/lane",
                "base-sha",
                "codex",
                4,
            )
            .unwrap();
        assert!(store
            .acknowledge_remote_dispatch(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                "remote-tab-1",
                5,
            )
            .unwrap());
        let report = TaskExecutionReportSnapshot {
            id: "remote-report-1".into(),
            message: Some("STATUS: in_review\nCHANGES: remote lane".into()),
            status: Some("in_review".into()),
            sha: Some("lane-sha".into()),
            reporter_tab_id: Some("remote-tab-1".into()),
            ..Default::default()
        };
        assert!(store
            .ingest_remote_dispatch_report(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                report.clone(),
                6,
            )
            .unwrap());
        assert!(!store
            .ingest_remote_dispatch_report(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                report,
                7,
            )
            .unwrap());
        let detail = store.show_task(&plan_id, &task_id).unwrap();
        assert_eq!(detail.task.status, TaskStatus::InReview);
        let attempt = &detail.task.execution_attempts[0];
        assert_eq!(attempt.mesh_peer.as_deref(), Some("peer-origin"));
        assert_eq!(attempt.remote_dispatch_state, Some(RemoteDispatchState::Reported));
        assert_eq!(attempt.reported_at, Some(6));
        assert_eq!(attempt.report.as_ref().unwrap().sha.as_deref(), Some("lane-sha"));
    }

    #[test]
    fn mesh_worker_peer_offline_marks_attempt_explicitly() {
        let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
        store
            .set_remote_dispatch_state(
                &plan_id,
                &task_id,
                &attempt_id,
                "peer-origin",
                RemoteDispatchState::PeerOffline,
                9,
            )
            .unwrap();
        let detail = store.show_task(&plan_id, &task_id).unwrap();
        assert_eq!(
            detail.task.execution_attempts[0].remote_dispatch_state,
            Some(RemoteDispatchState::PeerOffline)
        );
    }
}
