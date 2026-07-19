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

impl ServiceContext {
    pub(crate) fn bind_worker_plan_task_at_spawn(
        &self,
        params: &SpawnAgentParams,
        pane_id: &str,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        let (plan_id, _task_id) = bind_worker_task_to_pane(
            &self.plan_store,
            params.plan_id.as_deref(),
            task_ref,
            pane_id,
            epoch_ms(),
        )?;
        let now_ms = epoch_ms();
        let (title, session_id) = {
            let mux = self.mux.read();
            let title = mux.display_label_for_pane(pane_id);
            let session_id = mux
                .get_pane(pane_id)
                .and_then(|pane| pane.agent_session_id.clone());
            (title, session_id)
        };
        self.plan_store
            .update_attempt_executor_identity(
                &plan_id,
                task_ref,
                pane_id,
                Some(&title),
                session_id.as_deref(),
                now_ms,
            )
            .map_err(|e| e.to_string())?;
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
}
