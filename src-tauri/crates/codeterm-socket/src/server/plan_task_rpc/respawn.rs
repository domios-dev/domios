use super::super::plan_role_helpers::{record_spawned_attempt, RespawnAttemptRoleExt};
use super::*;
use codeterm_service_core::service::persist_attempt_executor_identity;

pub(super) fn respawn(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let (id, tid) = plan_refs::resolve_task(store, params)?;
    authorize_execution(svc, &id, p)?;
    let plan = store
        .get(&id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == tid)
        .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
    let source_id = get_string_param(params, "attempt_id")?;
    if let Some(source) = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == source_id && attempt.mesh_peer.is_some())
    {
        return super::super::plan_task_dispatch::remote_recovery::respawn_remote(
            params, svc, &id, &tid, source, now,
        );
    }
    if task.status == TaskStatus::Pending {
        codeterm_service_core::service::plan::review_gate::validate_dispatch_review_capacity(
            &plan, task,
        )
        .map_err(plan_rpc_err)?;
    }
    let body = p.ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    let request = codeterm_service_core::service::plan::task_attempts::TaskRespawnRequest {
        reasoning_effort: body
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_owned),
        attempt_id: get_string_param(params, "attempt_id")?,
        provider: body
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        model: body
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        parent_tab_id: body
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        workspace_id: body
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        title: body
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let mut draft = store
        .build_task_respawn_draft(&id, &tid, request)
        .map_err(plan_rpc_err)?;
    let original_task = task.clone();
    let original_plan_mode = plan.mode.clone();
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    };
    let lane_protection = codeterm_service_core::service::LaneProtection::for_dispatch_intent(
        &task.interruption.execution_intent,
    );
    let provenance = codeterm_service_core::service::plan::respawn_lane::verify_respawn_lane(
        &canonical_cwd,
        task,
        &draft,
    )
    .map_err(|error| JsonRpcError {
        code: -32603,
        message: format!("respawn lane verification failed: {error}"),
        data: None,
    })?;
    if let Some(provenance) = provenance.as_ref() {
        super::super::plan_task_dispatch::lane_safety::refuse_unsafe_mutating_write(
            &plan,
            &tid,
            &provenance.worktree_path,
            task.interruption.execution_intent.clone(),
        )?;
    }
    let parent_tab_id = draft
        .parent_tab_id
        .clone()
        .or_else(|| plan.owner.tab_id.clone())
        .filter(|parent| !parent.trim().is_empty())
        .ok_or(JsonRpcError {
            code: -32602,
            message: "respawn requires a parent Orchestrator tab".into(),
            data: None,
        })?;
    draft.parent_tab_id = Some(parent_tab_id.clone());
    let _respawn_reservation = store
        .reserve_task_respawn_startup(&id, &tid, &draft.source_attempt.id, &draft.attempt_id)
        .map_err(plan_rpc_err)?;
    svc.retire_respawn_executor(&draft).map_err(|error| JsonRpcError {
        code: -32603,
        message: error,
        data: None,
    })?;
    let mut spawn_params = super::super::plan_task_dispatch::respawn_worker_params(
        &draft,
        &task.name,
        task.lane.base.clone(),
        &parent_tab_id,
    );
    spawn_params.speed = body
        .get("speed")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(spawn_params.speed);
    let speed = spawn_params.speed.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let spawn = svc
        .spawn_agent_sync_with_ready(spawn_params, Some(ready_tx))
        .map_err(|e| JsonRpcError {
            code: -32603,
            message: e,
            data: None,
        })?;
    if let Some(speed) = speed.as_deref() {
        store
            .update_task_speed(&id, &tid, Some(speed), now)
            .map_err(plan_rpc_err)?;
    }
    draft.provider = spawn.agent.clone();
    draft.model = spawn.provider_resolved_model.clone();
    draft.reasoning_effort = svc.resolved_worker_launch_effort_for_tab(&spawn.tab_id);
    let created = record_spawned_attempt(svc, &spawn.tab_id, || {
        store.record_task_respawn_attempt_with_profile(&id, &tid, &spawn.tab_id, &draft, svc, now)
    })?;
    persist_attempt_executor_identity(store, svc, &id, &tid, &spawn.tab_id, &spawn.title, now)
        .map_err(plan_rpc_err)?;
    super::super::plan_task_dispatch::todo_seed::seed_acceptance_or_rollback(
        super::super::plan_task_dispatch::todo_seed::DispatchSeed {
            svc,
            store,
            plan_id: &id,
            task_id: &tid,
            parent_tab_id: &parent_tab_id,
            tab_id: &spawn.tab_id,
            attempt_id: &created.attempt.id,
            original_task: &original_task,
            original_plan_mode: &original_plan_mode,
            provisioned_lane: None,
            now,
        },
    )?;
    emit_plan_changed(svc, &id);
    let startup = super::super::plan_task_dispatch::DispatchStartup {
        svc: svc.clone(),
        plan_id: id.clone(),
        task_id: tid.clone(),
        tab_id: spawn.tab_id.clone(),
        startup_attempt_id: spawn.pty_id.clone(),
        attempt_id: created.attempt.id.clone(),
        parent_tab_id,
        worker_title: spawn.title.clone(),
        original_task,
        original_plan_mode,
        lane_for_commit: task.lane.clone(),
        canonical_project_path: canonical_cwd.clone(),
        provenance,
        provisioned_lane: None,
        lane_protection,
        spawn_command: format!(
            "domios plan task respawn {id} {tid} --attempt {}",
            draft.source_attempt.id
        ),
    };
    std::thread::spawn(move || {
        super::super::plan_task_dispatch::startup::run_startup(startup, ready_rx)
    });
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
        "tab_id": spawn.tab_id,
        "spawn": spawn,
        "attempt": created.attempt,
        "draft": draft
    }))
}
