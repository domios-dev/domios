use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::OrchestratorRef;
use crate::service::plan::mutations::goal_ops::GoalPatch;
use crate::service::plan::PlanError;
use crate::service::{ServiceState, TeamPlanCreationError};

use super::JsonRpcError;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

/// A list field the caller either did not mention (`None`, kept) or set. A single
/// empty entry is how a caller clears the list without a dedicated flag.
fn string_list_field(body: &Value, key: &str) -> Result<Option<Vec<String>>, JsonRpcError> {
    let Some(value) = body.get(key).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let items: Vec<String> =
        serde_json::from_value(value.clone()).map_err(|error| JsonRpcError {
            code: -32602,
            message: format!("invalid {key}: {error}"),
            data: None,
        })?;
    Ok(Some(
        items
            .into_iter()
            .filter(|item| !item.trim().is_empty())
            .collect(),
    ))
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
            let caller_tab = body.get("caller_tab_id").and_then(|v| v.as_str());
            // A caller passing only --vector used to wipe the vision, constraints and postulates it never named.
            let patch = GoalPatch {
                vector: body
                    .get("vector")
                    .and_then(|v| v.as_str())
                    .map(|value| value.replace('\n', " ")),
                vision: body
                    .get("vision")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                constraints: string_list_field(body, "constraints")?,
                postulates: string_list_field(body, "postulates")?,
            };
            let mut created_team = None;
            let id = match super::plan_refs::resolve_plan(store, params) {
                Ok(id) => id,
                Err(_err)
                    if caller_tab
                        .map(|tab| svc.tab_has_pending_paired_planner(tab))
                        .unwrap_or(false) =>
                {
                    // A plan minted here has no goal to keep fields from, so the vector is mandatory — refused BEFORE the plan is created, never after, or the refusal leaves a plan behind.
                    if patch.vector.is_none() {
                        return Err(plan_rpc_err(PlanError::GoalVectorRequired));
                    }
                    let title = body
                        .get("title")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_string)
                        .or_else(|| patch.vector.clone())
                        .unwrap_or_default();
                    let project = body
                        .get("project_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            std::env::current_dir()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default()
                        });
                    let owner = OrchestratorRef::from_tab(caller_tab.unwrap_or("unknown"));
                    let outcome = svc
                        .create_team_plan_for_orchestrator(
                            title,
                            project,
                            owner,
                            Vec::new(),
                            None,
                            epoch_ms(),
                        )
                        .map_err(|error| match error {
                            TeamPlanCreationError::Plan(error) => plan_rpc_err(error),
                            TeamPlanCreationError::Team(message) => JsonRpcError {
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
            let goal = store
                .patch_goal(&id, patch, epoch_ms())
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let mut resp = json!({ "ok": true, "goal": goal });
            if let Some(nudge) = store.get(&id).as_ref().and_then(|plan| {
                crate::service::plan::hints::planner_ownership_nudge(plan, caller_tab)
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
