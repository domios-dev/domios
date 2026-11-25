use codeterm_orchestration::traits::{HierarchyExt, RemoteTaskReportDelivery};
use codeterm_service_core::agent::reports::ReportModelIdentity;
use codeterm_service_core::plan::TaskExecutionReportSnapshot;
use codeterm_service_core::service::ServiceState;
use serde_json::{json, Value};

use super::super::plan::{emit_plan_changed, plan_rpc_err};
use super::super::{get_string_param, plan_refs, JsonRpcError};

pub(super) fn remote_ack(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let attempt_id = get_string_param(params, "attempt_id")?;
    let worker_tab_id = get_string_param(params, "worker_tab_id")?;
    let peer_id =
        get_string_param(params, "authenticated_mesh_peer_id").map_err(|_| JsonRpcError {
            code: -32003,
            message: "remote ACK requires authenticated mesh transport".into(),
            data: None,
        })?;
    let acknowledged = store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            &worker_tab_id,
            now,
        )
        .map_err(plan_rpc_err)?;
    svc.acknowledge_remote_worker_ack(&plan_id, &task_id, &attempt_id, &peer_id, &worker_tab_id);
    if acknowledged {
        emit_plan_changed(svc, &plan_id);
        notify_remote_worker_ack(
            svc,
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            &worker_tab_id,
            now,
        );
    }
    Ok(json!({
        "acknowledged": acknowledged,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
    }))
}

pub(super) fn remote_report(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let body = params.as_ref().ok_or_else(|| JsonRpcError {
        code: -32602,
        message: "remote report params required".into(),
        data: None,
    })?;
    let attempt_id = get_string_param(params, "attempt_id")?;
    let peer_id =
        get_string_param(params, "authenticated_mesh_peer_id").map_err(|_| JsonRpcError {
            code: -32003,
            message: "remote report requires authenticated mesh transport".into(),
            data: None,
        })?;
    let report: TaskExecutionReportSnapshot =
        serde_json::from_value(body.get("report").cloned().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "remote report snapshot is required".into(),
            data: None,
        })?)
        .map_err(|error| JsonRpcError {
            code: -32602,
            message: format!("invalid remote report snapshot: {error}"),
            data: None,
        })?;
    if report.id.trim().is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "remote report id is required for idempotency".into(),
            data: None,
        });
    }
    if report.status.as_deref() != Some("in_review") {
        return Err(JsonRpcError {
            code: -32602,
            message: "remote task reports must enter in_review for origin acceptance".into(),
            data: None,
        });
    }
    let reported = store
        .ingest_remote_dispatch_report(
            &plan_id,
            &task_id,
            &attempt_id,
            &peer_id,
            report.clone(),
            now,
        )
        .map_err(plan_rpc_err)?;
    if reported {
        emit_plan_changed(svc, &plan_id);
        notify_remote_worker_report(svc, &plan_id, &task_id, &attempt_id, &report);
    }
    Ok(json!({
        "reported": reported,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
    }))
}

fn notify_remote_worker_ack(
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    peer_id: &str,
    worker_tab_id: &str,
    now: u64,
) {
    let delivery = match svc
        .plan_store
        .remote_attempt_delivery_projection(plan_id, task_id, attempt_id)
    {
        Ok(Some(delivery)) => delivery,
        Ok(None) => return,
        Err(error) => {
            log::warn!(
                "[remote-ack] delivery metadata lookup failed plan={plan_id} task={task_id} attempt={attempt_id}: {error}"
            );
            return;
        }
    };
    let Some(parent_tab_id) = delivery.target_orchestrator_tab_id() else {
        return;
    };
    let title = delivery
        .executor_title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Worker");
    let payload = json!({
        "kind": "worker_ack",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": worker_tab_id,
        "attempt_id": attempt_id,
        "acknowledged_at": Some(now),
        "state": "in_progress",
        "peer_id": peer_id,
    });
    super::super::plan_task_dispatch::publish_startup_report(
        svc,
        parent_tab_id,
        worker_tab_id,
        title,
        plan_id,
        task_id,
        payload,
    );
}

fn notify_remote_worker_report(
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    report: &TaskExecutionReportSnapshot,
) {
    let delivery = match svc
        .plan_store
        .remote_attempt_delivery_projection(plan_id, task_id, attempt_id)
    {
        Ok(Some(delivery)) => delivery,
        Ok(None) => return,
        Err(error) => {
            log::warn!(
                "[remote-report] delivery metadata lookup failed plan={plan_id} task={task_id} attempt={attempt_id}: {error}"
            );
            return;
        }
    };
    let Some(target_tab_id) = delivery.target_orchestrator_tab_id() else {
        return;
    };
    let from_tab_id = report
        .reporter_tab_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(delivery.executor_tab_id.as_str());
    let from_title = delivery
        .executor_title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Worker");
    let actual_model = delivery
        .session_model
        .clone()
        .or_else(|| delivery.model.clone());
    let model_identity = (actual_model.is_some()
        || delivery.requested_model.is_some()
        || delivery.provider_resolved_model.is_some())
    .then(|| ReportModelIdentity {
        actual_model: actual_model.clone(),
        requested_model: delivery.requested_model.clone(),
        provider_resolved_model: delivery.provider_resolved_model.clone(),
        ..Default::default()
    });
    let model = actual_model
        .or_else(|| delivery.provider_resolved_model.clone())
        .or_else(|| delivery.requested_model.clone());
    (&**svc).deliver_remote_task_report(RemoteTaskReportDelivery {
        target_tab_id,
        from_tab_id,
        from_title,
        plan_id,
        task_id,
        attempt_id,
        branch: delivery.branch.clone(),
        base: delivery.base.clone(),
        model_identity,
        model,
        snapshot: report,
    });
}
