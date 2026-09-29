use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::ServiceState;

use super::super::plan::{emit_plan_changed, plan_rpc_err};
use super::super::JsonRpcError;
use super::{
    dispatch_with_spawn_approval_exemption, invalid_dispatch, owned_str,
    startup_wait_is_recoverable,
};

pub(crate) fn dispatch_unplanned(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    dispatch_unplanned_with_spawn(params, app, svc, now, |svc, spawn_params, ready_tx| {
        svc.spawn_agent_sync_with_ready(spawn_params, ready_tx)
    })
}

pub(super) fn dispatch_unplanned_with_spawn<S>(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
    spawn_agent: S,
) -> Result<Value, JsonRpcError>
where
    S: FnOnce(
        &ServiceState,
        crate::service::SpawnAgentParams,
        Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<crate::service::SpawnResult, String>,
{
    let p = params.as_ref();
    let reason = owned_str(p, "reason")
        .map(|reason| reason.trim().to_string())
        .filter(|reason| !reason.is_empty())
        .ok_or_else(|| invalid_dispatch("--unplanned requires a non-empty --reason"))?;
    if p.is_some_and(|params| {
        params
            .get("placement")
            .is_some_and(|placement| !placement.is_null() && placement != "isolated_lane")
            || params.get("canonical_writer").is_some()
            || params.get("authorized_gates").is_some()
    }) {
        return Err(invalid_dispatch(
            "unplanned Worker dispatch always uses an isolated lane: placement, canonical_writer and authorized_gates are refused",
        ));
    }
    let plan_id = super::super::plan_refs::resolve_plan(&svc.plan_store, params).map_err(|_| {
        invalid_dispatch(
            "unplanned Worker dispatch requires a bound Team Plan; set the Team goal first",
        )
    })?;
    let brief = owned_str(p, "brief").unwrap_or_default();
    let task_body = if brief.trim().is_empty() {
        format!("Complete the unplanned work: {reason}")
    } else {
        brief.clone()
    };
    let acceptance = task_body.clone();
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: owned_str(p, "caller_tab_id"),
        session_id: owned_str(p, "session_id"),
        binding_nonce: owned_str(p, "session_binding_nonce"),
    };
    svc.authorize_plan_orchestrator(&plan_id, &caller)
        .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({ "type": "plan_orchestrator_authorization_required" })),
        })?;
    let parent_task_id = caller
        .tab_id
        .as_deref()
        .and_then(|tab| svc.plan_store.task_binding_for_tab(tab))
        .filter(|(bound_plan, _)| bound_plan == &plan_id)
        .map(|(_, task)| task);
    let plan = svc.plan_store.get(&plan_id).ok_or_else(|| JsonRpcError {
        code: -32004,
        message: "unplanned task plan disappeared before creation".into(),
        data: Some(json!({ "type": "plan_not_found" })),
    })?;
    let inherited_section_id = parent_task_id
        .as_deref()
        .and_then(|task_id| plan.tasks.iter().find(|task| task.id == task_id))
        .and_then(|task| task.section_ids.first().cloned());
    let section_id = match inherited_section_id
        .or_else(|| plan.sections.first().map(|section| section.id.clone()))
    {
        Some(section_id) => section_id,
        None => svc
            .plan_store
            .add_section(
                &plan_id,
                "Unplanned work",
                "Urgent work dispatched outside the planned task list.",
                None,
                &[],
                None,
                None,
                now,
            )
            .map_err(plan_rpc_err)?,
    };
    let task_id = svc
        .plan_store
        .add_task(
            &plan_id,
            crate::service::plan::NewTask {
                name: format!("unplanned: {reason}"),
                kind: "unplanned".into(),
                body: task_body,
                executor_role: Some("Worker".into()),
                parent_task_id,
                tags: vec!["unplanned".into()],
                section_ids: vec![section_id],
                acceptance_criteria: vec![acceptance.clone()],
                correctness: Some(acceptance),
                test_plan: Some(
                    "The Orchestrator verifies the reported result against the brief.".into(),
                ),
                ..Default::default()
            },
            now,
        )
        .map_err(plan_rpc_err)?;
    let mut dispatch_params = p.cloned().unwrap_or_default();
    dispatch_params["plan_id"] = json!(plan_id.clone());
    dispatch_params["task_id"] = json!(task_id.clone());
    dispatch_params["execution_intent"] = json!("mutating");
    let dispatched = dispatch_with_spawn_approval_exemption(
        &Some(dispatch_params),
        app,
        svc,
        now,
        Some(&task_id),
        spawn_agent,
        || {},
    );
    if let Err(error) = &dispatched {
        if !startup_wait_is_recoverable(error) {
            if let Err(cleanup_error) = svc.plan_store.rm_task(&plan_id, &task_id, now) {
                return Err(JsonRpcError {
                    code: error.code,
                    message: format!(
                        "{}; unplanned task cleanup failed: {cleanup_error}",
                        error.message
                    ),
                    data: Some(json!({
                        "type": "unplanned_dispatch_rollback_failed",
                        "dispatch_error": error.data,
                        "orphan_task": { "plan_id": plan_id, "task_id": task_id },
                        "cleanup_error": cleanup_error.to_string(),
                    })),
                });
            }
        }
    }
    emit_plan_changed(app, &plan_id);
    dispatched
}
