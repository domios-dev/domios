//! Worker spawn binds a named plan task (`--plan-task`) to the new pane.

use crate::service::plan::PlanStore;
use crate::service::{ServiceContext, SpawnAgentParams};

pub(crate) fn worker_spawn_plan_task_ref(params: &SpawnAgentParams) -> Option<&str> {
    params
        .plan_task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn validate_worker_plan_task_spawn(params: &SpawnAgentParams) -> Result<(), String> {
    if worker_spawn_plan_task_ref(params).is_some() && params.role.as_deref() != Some("Worker") {
        return Err("--plan-task is only valid with --role Worker".into());
    }
    Ok(())
}

/// Reject a WIP-gate-blocked dispatch before any pane is provisioned, so
/// there is nothing left to roll back.
pub(crate) fn preflight_worker_plan_task_wip_gate(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
) -> Result<(), String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan = store
        .get(&resolved.plan_id)
        .ok_or_else(|| "plan not found".to_string())?;
    let task = plan
        .tasks
        .iter()
        .find(|t| t.id == resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
    if task.status != crate::plan::TaskStatus::Pending {
        return Ok(());
    }
    crate::service::plan::review_gate::validate_dispatch_review_capacity(&plan, task)
        .map_err(|error| error.to_string())
}

pub(crate) fn bind_worker_task_to_pane(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
    pane_id: &str,
    now_ms: u64,
) -> Result<(String, String), String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan_id = resolved.plan_id;
    let task_id = resolved.task_id;
    store
        .assign_task_with_role(&plan_id, &task_id, pane_id, None, Some("worker"), now_ms)
        .map_err(|e| e.to_string())?;
    if let Err(e) = store.update_task(
        &plan_id,
        &task_id,
        None,
        None,
        None,
        None,
        Some("in_progress"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        None,
        now_ms,
    ) {
        let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
        return Err(e.to_string());
    }
    Ok((plan_id, task_id))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record executor identity on the just-bound attempt; on failure, revert the
/// task/lane binding so a partial bind never lingers as live plan state.
fn record_executor_identity_or_revert(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    task_ref: &str,
    pane_id: &str,
    title: &str,
    session_id: Option<&str>,
    now_ms: u64,
) -> Result<(), String> {
    if let Err(e) = store.update_attempt_executor_identity(
        plan_id,
        task_ref,
        pane_id,
        Some(title),
        session_id,
        now_ms,
    ) {
        let _ = store.revert_task_dispatch(plan_id, task_id, now_ms);
        return Err(e.to_string());
    }
    Ok(())
}

/// Bind the pane and executor identity as one compensated transaction.
/// Any failure reverts plan state and closes the provisioned pane.
pub(crate) fn bind_worker_plan_task_transaction(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
    pane_id: &str,
    title: &str,
    session_id: Option<&str>,
    now_ms: u64,
    close_pane: impl FnOnce(),
) -> Result<String, String> {
    let (plan_id, task_id) =
        match bind_worker_task_to_pane(store, plan_id, task_ref, pane_id, now_ms) {
            Ok(ids) => ids,
            Err(e) => {
                close_pane();
                return Err(e);
            }
        };
    if let Err(e) = record_executor_identity_or_revert(
        store, &plan_id, &task_id, task_ref, pane_id, title, session_id, now_ms,
    ) {
        close_pane();
        return Err(e);
    }
    Ok(plan_id)
}

impl ServiceContext {
    /// Preflight the review WIP gate before any pane gets provisioned.
    pub(crate) fn preflight_worker_plan_task_spawn(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        preflight_worker_plan_task_wip_gate(&self.plan_store, params.plan_id.as_deref(), task_ref)
    }

    /// Bind `--plan-task` to the pane, compensating fully on any failure —
    /// see `bind_worker_plan_task_transaction`.
    pub(crate) fn bind_worker_plan_task_or_rollback_pane(
        &self,
        params: &SpawnAgentParams,
        pane_id: &str,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        let now_ms = epoch_ms();
        let (title, session_id) = {
            let mux = self.mux.read();
            let title = mux.display_label_for_pane(pane_id);
            let session_id = mux
                .get_pane(pane_id)
                .and_then(|pane| pane.agent_session_id.clone());
            (title, session_id)
        };
        let plan_id = bind_worker_plan_task_transaction(
            &self.plan_store,
            params.plan_id.as_deref(),
            task_ref,
            pane_id,
            &title,
            session_id.as_deref(),
            now_ms,
            || {
                let _ = self.close_pane(pane_id);
            },
        )?;
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, PlanMode, TaskStatus};
    use crate::service::plan::{NewTask, PlanStore};

    fn store_with_task(dir: &std::path::Path) -> (PlanStore, String, String) {
        let store = PlanStore::new_for_test(dir);
        let plan = store
            .create_plan_for_team(
                "Spawn bind".into(),
                dir.display().to_string(),
                OrchestratorRef::from_pane("orch"),
                "team-orch".into(),
                vec![],
                1,
            )
            .unwrap();
        let mut p = store.get(&plan.id).unwrap();
        p.mode = PlanMode::Approved;
        store.save(&mut p, 2).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "bind-at-spawn".into(),
                    handle: Some("spawn-bind".into()),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        (store, plan.id, task_id)
    }

    #[test]
    fn bind_sets_executor_and_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        bind_worker_task_to_pane(&store, Some(&plan_id), "spawn-bind", "pane-worker", 10).unwrap();
        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.assigned_pane_id.as_deref(), Some("pane-worker"));
        assert_eq!(task.lane.pane_id.as_deref(), Some("pane-worker"));
        assert_eq!(task.taken_by_role.as_deref(), Some("worker"));
    }

    #[test]
    fn validate_rejects_non_worker() {
        let err = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Orchestrator".into()),
            plan_task: Some("spawn-bind".into()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("--plan-task"));
    }

    fn block_review_wip(store: &PlanStore, plan_id: &str) {
        let mut p = store.get(plan_id).unwrap();
        p.review_policy.wip_limit = 0;
        store.save(&mut p, 4).unwrap();
    }

    #[test]
    fn preflight_rejects_before_any_pane_when_wip_gate_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let err =
            preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap_err();
        assert!(err.contains("review"), "got: {err}");
    }

    #[test]
    fn preflight_allows_dispatch_when_gate_is_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    #[test]
    fn spawn_preflight_requires_daemon_owned_capacity_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let mut plan = store.get(&plan_id).unwrap();
        plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.kind = "review".into();
        task.tags.push("review-capacity".into());
        store.save(&mut plan, 4).unwrap();
        let error =
            preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap_err();
        assert!(error.contains("review"), "{error}");

        store
            .grant_review_capacity(
                &plan_id,
                &task_id,
                crate::plan::ReviewCapacityClass::Review,
                crate::plan::AgentRef::from_pane("orch"),
                5,
            )
            .unwrap();
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    // Reproduces the incident: `assign_task_with_role` (step 1) already binds
    // the pane before `update_task(in_progress)` (step 2) hits the WIP gate.
    #[test]
    fn wip_gate_failure_after_pane_bind_leaves_zero_residual_task_lane_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let err = bind_worker_task_to_pane(&store, Some(&plan_id), "spawn-bind", "pane-worker", 10)
            .unwrap_err();
        assert!(err.contains("review"), "got: {err}");

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_pane_id, None);
        assert_eq!(task.lane.pane_id, None);
    }

    // A mismatched pane_id models any bug where the two steps disagree on
    // which pane the attempt lives on; the failed record must revert the bind.
    #[test]
    fn identity_update_failure_after_bind_reverts_task_and_lane_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        bind_worker_task_to_pane(&store, Some(&plan_id), "spawn-bind", "pane-worker", 10).unwrap();

        let err = record_executor_identity_or_revert(
            &store,
            &plan_id,
            &task_id,
            "spawn-bind",
            "pane-mismatched",
            "Ada",
            None,
            11,
        )
        .unwrap_err();
        assert!(!err.is_empty());

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_pane_id, None);
        assert_eq!(task.lane.pane_id, None);
    }

    // Proven via the WIP-gate path — the failure reliably triggerable here
    // without dependency-injecting the identity step itself.
    #[test]
    fn transaction_closes_pane_and_reverts_state_on_any_post_provision_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let mut pane_closed = false;
        let err = bind_worker_plan_task_transaction(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "pane-worker",
            "Ada",
            None,
            10,
            || pane_closed = true,
        )
        .unwrap_err();
        assert!(err.contains("review"), "got: {err}");
        assert!(
            pane_closed,
            "the just-created Tab must be closed on any post-provision failure"
        );

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_pane_id, None);
        assert_eq!(task.lane.pane_id, None);
    }
}
