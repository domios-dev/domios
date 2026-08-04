use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::plan::TaskCompetitionMetadata;
use crate::service::{ServiceState, SpawnIdentityProvenance};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_fields::tab_role;
use super::plan_refs;
use super::plan_role_helpers::record_spawned_attempt;
use super::{get_string_param, JsonRpcError};

pub(super) fn metadata_field(
    p: Option<&Value>,
) -> Result<Option<TaskCompetitionMetadata>, JsonRpcError> {
    p.and_then(|x| x.get("competition"))
        .cloned()
        .map(|value| serde_json::from_value(value).map_err(invalid_metadata))
        .transpose()
}

pub(super) fn update_field(
    p: Option<&Value>,
) -> Result<Option<crate::service::plan::competition::TaskCompetitionUpdate>, JsonRpcError> {
    p.and_then(|x| x.get("competition"))
        .cloned()
        .map(|value| serde_json::from_value(value).map_err(invalid_update))
        .transpose()
}

fn invalid_metadata(e: serde_json::Error) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: format!("invalid competition metadata: {e}"),
        data: None,
    }
}

fn invalid_update(e: serde_json::Error) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: format!("invalid competition update: {e}"),
        data: None,
    }
}

fn preflight_compete_review_capacity(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
) -> Result<(), crate::service::plan::PlanError> {
    let plan = store
        .get(plan_id)
        .ok_or(crate::service::plan::PlanError::NotFound)?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or(crate::service::plan::PlanError::TaskNotFound)?;
    if task.status == crate::plan::TaskStatus::Pending {
        crate::service::plan::review_gate::validate_dispatch_review_capacity(&plan, task)?;
    }
    Ok(())
}

pub(super) fn compete(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    super::plan_task_rpc::authorize_execution(svc, &id, p)?;
    preflight_compete_review_capacity(store, &id, &tid).map_err(plan_rpc_err)?;
    let body = p.ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    let request = crate::service::plan::task_attempts::TaskCompeteRequest {
        attempts: body
            .get("attempts")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        provider: body
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        models: body
            .get("models")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        parent_tab_id: body
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        workspace_id: body
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let drafts = store
        .build_task_compete_drafts(&id, &tid, request)
        .map_err(plan_rpc_err)?;
    let plan = store
        .get(&id)
        .ok_or_else(|| plan_rpc_err(crate::service::plan::PlanError::NotFound))?;
    let mut attempts = Vec::new();
    let mut spawns = Vec::new();
    for draft in drafts {
        let parent_tab_id = draft
            .parent_tab_id
            .clone()
            .or_else(|| plan.owner.tab_id.clone())
            .ok_or(JsonRpcError {
                code: -32602,
                message: "competition requires a parent Orchestrator tab".into(),
                data: None,
            })?;
        let spawn_params = crate::service::SpawnAgentParams {
            agent: Some(draft.provider.clone()),
            cwd: Some(draft.cwd.clone()),
            title: draft.title.clone(),
            title_provenance: Some(SpawnIdentityProvenance::Suggested),
            role: Some("Worker".into()),
            parent_tab_id: Some(parent_tab_id.clone()),
            task: Some(draft.task_prompt.clone()),
            workspace_id: draft.workspace_id.clone(),
            model: draft.model.clone(),
            planned_model: draft.planned_model.clone(),
            spawned_by_tab_id: Some(parent_tab_id.clone()),
            ..Default::default()
        };
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let spawn = svc
            .spawn_agent_sync_with_ready(spawn_params, Some(ready_tx))
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
                data: None,
            })?;
        let created = record_spawned_attempt(svc, &spawn.tab_id, || {
            store.record_task_compete_attempt(
                &id,
                &tid,
                &spawn.tab_id,
                &draft,
                tab_role(svc, &spawn.tab_id),
                now,
            )
        })?;
        let svc_for_ready = svc.clone();
        let app_for_ready = app.clone();
        let plan_id = id.clone();
        let task_id = tid.clone();
        let tab_id = spawn.tab_id.clone();
        let attempt_id = created.attempt.id.clone();
        let worker_title = spawn.title.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            let runtime_ready = super::plan_task_dispatch::await_agent_ready(ready_rx, deadline)
                .and_then(|_| {
                    super::plan_task_dispatch::wait_for_tab_session(
                        &svc_for_ready,
                        &tab_id,
                        deadline,
                    )
                });
            match runtime_ready {
                Ok(session_id) => {
                    if let Err(error) = svc_for_ready.plan_store.mark_task_attempt_runtime_ready(
                        &plan_id,
                        &task_id,
                        &tab_id,
                        &attempt_id,
                        &session_id,
                        epoch_ms(),
                    ) {
                        super::plan_task_dispatch::publish_startup_report(
                            &svc_for_ready,
                            &parent_tab_id,
                            &tab_id,
                            &worker_title,
                            &plan_id,
                            &task_id,
                            json!({
                                "kind": "dispatch_startup_failed",
                                "stage": "task_commit",
                                "error": error.to_string(),
                            }),
                        );
                        return;
                    }
                    super::plan_task_dispatch::arm_worker_ack_deadline(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        &attempt_id,
                        format!("codeterm plan task compete {plan_id} {task_id}"),
                    );
                    emit_plan_changed(&app_for_ready, &plan_id);
                    super::plan_task_dispatch::publish_startup_report(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        json!({
                            "kind": "dispatch_ready",
                            "plan_id": plan_id,
                            "task_id": task_id,
                            "tab_id": tab_id,
                            "attempt_id": attempt_id,
                            "session_id": session_id,
                        }),
                    );
                }
                Err(error) => {
                    let _ = svc_for_ready.close_tab(&tab_id);
                    emit_plan_changed(&app_for_ready, &plan_id);
                    super::plan_task_dispatch::publish_startup_report(
                        &svc_for_ready,
                        &parent_tab_id,
                        &tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        json!({
                            "kind": "dispatch_startup_failed",
                            "stage": "agent_ready",
                            "error": error,
                        }),
                    );
                }
            }
        });
        attempts.push(created.attempt);
        spawns.push(spawn);
    }
    emit_plan_changed(app, &id);
    let group = attempts
        .first()
        .and_then(|attempt| attempt.competition_group_id.clone())
        .unwrap_or_default();
    Ok(json!({
        "ok": true,
        "accepted": true,
        "state": "awaiting_dispatch_ready",
        "readiness": {
            "ready": false,
            "awaiting_event": "dispatch_ready",
        },
        "plan_id": id,
        "task_id": tid,
        "competition_group_id": group,
        "attempts": attempts,
        "spawns": spawns
    }))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRef, OrchestratorRef, Plan, ReviewCapacityClass};
    use crate::service::plan::{NewTask, PlanError, PlanStore};

    #[test]
    fn compete_cannot_bypass_review_debt_with_spoofed_task_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "competition".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "spoofed review".into(),
                    kind: "review".into(),
                    tags: vec!["review-capacity".into(), "canonical-gate".into()],
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let mut persisted = store.get(&plan.id).unwrap();
        persisted.review_policy.credit = -(persisted.review_policy.wip_limit as i64);
        store.save(&mut persisted, 3).unwrap();

        assert!(matches!(
            preflight_compete_review_capacity(&store, &plan.id, &task_id),
            Err(PlanError::ReviewWipExceeded { .. })
        ));
        store
            .grant_review_capacity(
                &plan.id,
                &task_id,
                ReviewCapacityClass::Review,
                AgentRef::from_tab("orch"),
                4,
            )
            .unwrap();
        preflight_compete_review_capacity(&store, &plan.id, &task_id).unwrap();
    }
}

pub(super) fn judge(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    let attempt_id = get_string_param(params, "attempt_id")?;
    let verdict = get_string_param(params, "verdict")?;
    let reason = p
        .and_then(|x| x.get("reason"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    let winner = p.and_then(|x| x.get("winner")).and_then(|v| v.as_bool()) == Some(true);
    let judged = store
        .judge_task_attempt(&id, &tid, &attempt_id, &verdict, reason, winner, now)
        .map_err(plan_rpc_err)?;
    emit_plan_changed(app, &id);
    Ok(json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "attempt": judged.attempt
    }))
}
