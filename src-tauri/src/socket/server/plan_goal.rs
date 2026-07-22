use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{Goal, OrchestratorRef};
use crate::service::plan::PlanError;
use crate::service::{FirstPlanTeamError, ServiceState};

use super::{get_string_param, JsonRpcError};

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    match method {
        "plan.goal.set" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let caller_pane = body.get("caller_pane_id").and_then(|v| v.as_str());
            let goal = Goal {
                vector: get_string_param(params, "vector")?.replace('\n', " "),
                vision: body
                    .get("vision")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                constraints: body
                    .get("constraints")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default(),
                postulates: body
                    .get("postulates")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default(),
            };
            let mut created_team = None;
            let id = match super::plan_refs::resolve_plan(store, params) {
                Ok(id) => id,
                Err(_err)
                    if caller_pane
                        .map(|pane| svc.pane_has_pending_paired_planner(pane))
                        .unwrap_or(false) =>
                {
                    let title = body
                        .get("title")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| goal.vector.clone());
                    let project = body
                        .get("project_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            std::env::current_dir()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default()
                        });
                    let owner = OrchestratorRef::from_pane(caller_pane.unwrap_or("unknown"));
                    let outcome = svc
                        .create_first_plan_for_orchestrator(
                            title,
                            project,
                            owner,
                            Vec::new(),
                            None,
                            epoch_ms(),
                        )
                        .map_err(|error| match error {
                            FirstPlanTeamError::Plan(error) => plan_rpc_err(error),
                            FirstPlanTeamError::Team(message) => JsonRpcError {
                                code: -32602,
                                message,
                                data: None,
                            },
                        })?;
                    let plan = outcome.plan.clone();
                    created_team = Some(outcome);
                    let _ = svc.bind_pending_paired_planner(&plan.id, &plan.owner, epoch_ms());
                    plan.id
                }
                Err(err) => return Err(err),
            };
            store
                .set_goal(&id, goal, epoch_ms())
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let mut resp = json!({ "ok": true });
            if let Some(nudge) = store.get(&id).as_ref().and_then(|plan| {
                crate::service::plan::hints::planner_ownership_nudge(plan, caller_pane)
            }) {
                resp["_planner_nudge"] = json!(nudge);
            }
            if let Some(outcome) = created_team {
                outcome.apply_to_response(&mut resp);
            }
            Ok(resp)
        }
        "plan.goal.show" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let plan = store
                .get(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(json!({ "plan_id": id, "goal": plan.goal }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
