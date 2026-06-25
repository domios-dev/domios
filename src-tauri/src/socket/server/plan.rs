use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{AgentRef, OrchestratorRef, PlanStatus, TaskLaneMetadata, TaskStatus};
use crate::service::plan::batch::BatchRequest;
use crate::service::plan::task_queries::TaskListFilter;
use crate::service::plan::{hints, NewTask, PlanError, TaskLaneUpdate};
use crate::service::ServiceState;

use super::plan_fields::{
    groups_from_csv, kinds_from_csv, optional_string_field, pane_and_ancestors, pane_role,
    string_array_field,
};
use super::plan_linking::{csv_string_field, owner_from_params, plan_with_linking, tags_field};
use super::{get_string_param, JsonRpcError};

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    let data = e.to_json();
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(data),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

fn lane_metadata_field(p: Option<&Value>) -> Result<TaskLaneMetadata, JsonRpcError> {
    p.and_then(|x| x.get("lane"))
        .map(|v| {
            serde_json::from_value(v.clone()).map_err(|e| JsonRpcError {
                code: -32602,
                message: format!("invalid lane metadata: {e}"),
                data: None,
            })
        })
        .transpose()
        .map(|lane| lane.unwrap_or_default())
}

fn lane_update_field(p: Option<&Value>) -> Result<Option<TaskLaneUpdate>, JsonRpcError> {
    let Some(lane) = p.and_then(|x| x.get("lane")) else {
        return Ok(None);
    };
    let mut update = TaskLaneUpdate::default();
    update.pane_id = lane.get("pane_id").map(|v| v.as_str().map(str::to_string));
    update.workspace_id = lane
        .get("workspace_id")
        .map(|v| v.as_str().map(str::to_string));
    update.workspace_name = lane
        .get("workspace_name")
        .map(|v| v.as_str().map(str::to_string));
    update.worktree_path = lane
        .get("worktree_path")
        .map(|v| v.as_str().map(str::to_string));
    update.branch = lane.get("branch").map(|v| v.as_str().map(str::to_string));
    update.base = lane.get("base").map(|v| v.as_str().map(str::to_string));
    update.destination = lane
        .get("destination")
        .map(|v| v.as_str().map(str::to_string));
    update.cleanup_status = lane
        .get("cleanup_status")
        .map(|v| v.as_str().map(str::to_string));
    if lane.get("cleanup_hints").is_some() {
        update.cleanup_hints = Some(
            serde_json::from_value(lane.get("cleanup_hints").cloned().unwrap_or(Value::Null))
                .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid cleanup_hints: {e}"),
                    data: None,
                })?,
        );
    }
    Ok((!update.is_empty()).then_some(update))
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    if method.starts_with("plan.handoff.") {
        return super::plan_handoff::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.heartbeat.") {
        return super::plan_heartbeat::dispatch(method, params, app, svc);
    }
    if method.starts_with("plan.goal.") {
        return super::plan_goal::dispatch(method, params, app, svc);
    }
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = epoch_ms();

    match method {
        "plan.batch" => {
            let id = get_string_param(params, "plan_id")?;
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request = BatchRequest {
                key: body.get("key").and_then(|v| v.as_str()).map(str::to_string),
                ops: body
                    .get("ops")
                    .cloned()
                    .ok_or(JsonRpcError {
                        code: -32602,
                        message: "Missing required parameter: ops".to_string(),
                        data: None,
                    })
                    .and_then(|v| {
                        serde_json::from_value(v).map_err(|e| JsonRpcError {
                            code: -32602,
                            message: format!("invalid batch ops: {e}"),
                            data: None,
                        })
                    })?,
            };
            let result = store.apply_batch(&id, request, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "plan.create" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let title = get_string_param(params, "title")?;
            let project = body
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                });
            let kinds = body
                .get("kinds")
                .and_then(|v| v.as_str())
                .map(kinds_from_csv)
                .unwrap_or_default();
            let from_spec = body
                .get("from_spec")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            let plan = if let Some(spec_id) = from_spec.clone() {
                store
                    .create_plan_from_spec(&spec_id, title, owner_from_params(body), kinds, now)
                    .map_err(plan_rpc_err)?
            } else {
                store
                    .create_plan(title, project, owner_from_params(body), kinds, now)
                    .map_err(plan_rpc_err)?
            };
            let plan_id = plan.id.clone();
            let empty_sections = plan.sections.is_empty();
            let mut response = json!({
                "plan_id": plan_id.clone(),
                "plan": plan,
                "from_spec": from_spec.unwrap_or_default()
            });
            if empty_sections {
                response["_hints"] = json!([hints::plan_sections_hint(&plan_id)]);
            }
            emit_plan_changed(app, &plan_id);
            Ok(response)
        }

        "plan.list" => {
            let want =
                if let Some(project) = p.and_then(|x| x.get("project")).and_then(|v| v.as_str()) {
                    Some(
                        svc.ensure_plan_project_loaded(std::path::Path::new(project))
                            .map_err(plan_rpc_err)?
                            .display()
                            .to_string(),
                    )
                } else {
                    None
                };
            let mut entries = store.list_index();
            if let Some(want) = &want {
                entries.retain(|e| {
                    store
                        .get(&e.plan_id)
                        .map(|pl| pl.project_path == *want)
                        .unwrap_or(false)
                });
            }
            if p.and_then(|x| x.get("unowned")).and_then(|v| v.as_bool()) == Some(true) {
                entries.retain(|e| e.owner.pane_id.is_none() && e.owner.session_id.is_none());
            }
            if let Some(status) = p.and_then(|x| x.get("status")).and_then(|v| v.as_str()) {
                entries.retain(|e| match status {
                    "active" => e.status == PlanStatus::Active,
                    "archived" => e.status == PlanStatus::Archived,
                    _ => true,
                });
            }
            Ok(json!({ "plans": entries }))
        }

        "plan.related" => {
            let id = get_string_param(params, "id")?;
            let depth = p
                .and_then(|x| x.get("depth"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            Ok(json!({
                "id": id,
                "depth": depth,
                "groups": store.related_units(&id),
            }))
        }

        "plan.reindex" => {
            let project = get_string_param(params, "project")?;
            let project_root = svc
                .ensure_plan_project_loaded(std::path::Path::new(&project))
                .map_err(plan_rpc_err)?;
            let result = store
                .reindex_project_index(&project_root)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            Ok(json!(result))
        }

        "task.list" => {
            let caller = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mine = p.and_then(|x| x.get("mine")).and_then(|v| v.as_bool()) == Some(true);
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|status| {
                    serde_json::from_value::<TaskStatus>(Value::String(status.into()))
                        .map_err(|_| plan_rpc_err(PlanError::BadStatus))
                })
                .transpose()?;
            let tasks = store.list_tasks(TaskListFilter {
                project_path: p
                    .and_then(|x| x.get("project"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                active_only: status.is_none()
                    && p.and_then(|x| x.get("active")).and_then(|v| v.as_bool()) != Some(false),
                mine_panes: if mine {
                    caller
                        .map(|id| pane_and_ancestors(svc, id))
                        .unwrap_or_default()
                } else {
                    Default::default()
                },
                unassigned: p
                    .and_then(|x| x.get("unassigned"))
                    .and_then(|v| v.as_bool())
                    == Some(true),
                plan_id: p
                    .and_then(|x| x.get("plan_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                status,
            });
            Ok(json!({ "tasks": tasks }))
        }

        "task.show" => {
            let task_id = get_string_param(params, "task_id")?;
            let detail = store
                .show_task(&task_id)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
            Ok(serde_json::to_value(detail).unwrap_or_default())
        }

        "task.take" => {
            let task_id = get_string_param(params, "task_id")?;
            let pane_id = get_string_param(params, "caller_pane_id")?;
            let role = pane_role(svc, &pane_id);
            let plan_id = store
                .set_task_assignment(&task_id, Some(&pane_id), role, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }))
        }

        "task.drop" => {
            let task_id = get_string_param(params, "task_id")?;
            let plan_id = store
                .set_task_assignment(&task_id, None, None, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }))
        }

        "task.add" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let project = body
                .get("project")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                });
            let owner = owner_from_params(body);
            let assign_pane = body
                .get("assign_pane")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let assign_role = assign_pane.as_deref().and_then(|pane| pane_role(svc, pane));
            let has_model = body
                .get("model")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let has_why = body
                .get("why")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let acceptance_criteria = string_array_field(p, "acceptance_criteria");
            let has_criteria = acceptance_criteria.iter().any(|s| !s.trim().is_empty());
            let test_plan = body
                .get("test_plan")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let correctness = body
                .get("correctness")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let (plan_id, task_id) = store
                .add_inbox_task(
                    &project,
                    owner,
                    NewTask {
                        name: get_string_param(params, "name")?,
                        kind: body
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("task")
                            .into(),
                        model: body
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        model_rationale: body
                            .get("why")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        acceptance_criteria,
                        test_plan,
                        correctness,
                        body: body
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .into(),
                        intent: body
                            .get("intent")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        assigned_pane_id: assign_pane.clone(),
                        ..Default::default()
                    },
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(pane) = assign_pane.as_deref() {
                store
                    .set_task_assignment(&task_id, Some(pane), assign_role, now)
                    .map_err(plan_rpc_err)?;
            }
            emit_plan_changed(app, &plan_id);
            let mut response_hints = hints::task_create_hints(
                &plan_id,
                &task_id,
                false,
                has_model,
                has_why,
                has_criteria,
                has_test_plan,
                has_correctness,
            );
            if let Some(hint) = hints::maybe_orchestration_reminder() {
                response_hints.push(hint);
            }
            Ok(json!({
                "plan_id": plan_id.clone(),
                "task_id": task_id.clone(),
                "_hints": response_hints
            }))
        }

        "plan.get" => {
            let id = get_string_param(params, "plan_id")?;
            let plan = store
                .get(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(plan_with_linking(&plan))
        }

        "plan.spec" => {
            let id = get_string_param(params, "plan_id")?;
            let spec = store
                .spec_markdown(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(json!({ "plan_id": id, "spec": spec }))
        }

        "plan.files" => {
            let id = get_string_param(params, "plan_id")?;
            let manifest = store.file_manifest(&id).map_err(plan_rpc_err)?;
            Ok(serde_json::to_value(manifest).unwrap_or_default())
        }

        "plan.tags" => {
            let id = get_string_param(params, "plan_id")?;
            let plan = store
                .get(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(
                serde_json::to_value(crate::service::plan::tag_model::tag_report(&plan))
                    .unwrap_or_default(),
            )
        }

        "plan.update" => {
            let id = get_string_param(params, "plan_id")?;
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|s| serde_json::from_value::<PlanStatus>(Value::String(s.into())))
                .transpose()
                .map_err(|_| plan_rpc_err(PlanError::BadStatus))?;
            store
                .update_plan(
                    &id,
                    p.and_then(|x| x.get("title")).and_then(|v| v.as_str()),
                    status,
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.archive" => {
            let id = get_string_param(params, "plan_id")?;
            store.archive(&id, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.delete" => {
            let id = get_string_param(params, "plan_id")?;
            store.delete(&id).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.transfer" => {
            let id = get_string_param(params, "plan_id")?;
            let to: OrchestratorRef = p
                .and_then(|x| x.get("to"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or(JsonRpcError {
                    code: -32602,
                    message: "to required".into(),
                    data: None,
                })?;
            let reason = p
                .and_then(|x| x.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            store.transfer(&id, to, reason, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.disown" => {
            let id = get_string_param(params, "plan_id")?;
            store.disown(&id, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.claim" => {
            let id = get_string_param(params, "plan_id")?;
            let params_val = p.cloned().unwrap_or(json!({}));
            let owner: OrchestratorRef = params_val
                .get("owner")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| owner_from_params(&params_val));
            store.claim(&id, owner, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.ingest" => {
            let id = get_string_param(params, "plan_id")?;
            let markdown = get_string_param(params, "markdown")?;
            let result = store
                .ingest_plan_sections(&id, &markdown, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(serde_json::to_value(result).unwrap_or_else(|_| json!({ "sections": [] })))
        }

        "plan.section.add" => {
            let id = get_string_param(params, "plan_id")?;
            let title =
                get_string_param(params, "title").or_else(|_| get_string_param(params, "name"))?;
            let body = p
                .and_then(|x| x.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let parent = p.and_then(|x| x.get("parent")).and_then(|v| v.as_str());
            let sid = store
                .add_section(
                    &id,
                    &title,
                    body,
                    parent,
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "section_id": sid }))
        }

        "plan.section.update" => {
            let id = get_string_param(params, "plan_id")?;
            let sid = get_string_param(params, "section_id")?;
            store
                .update_section(
                    &id,
                    &sid,
                    p.and_then(|x| x.get("title").or_else(|| x.get("name")))
                        .and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.section.rm" => {
            let id = get_string_param(params, "plan_id")?;
            let sid = get_string_param(params, "section_id")?;
            store.rm_section(&id, &sid, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.kinds.set" => {
            let id = get_string_param(params, "plan_id")?;
            let kinds = get_string_param(params, "kinds")?;
            store
                .set_kinds(&id, kinds_from_csv(&kinds), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.coverage" => {
            let id = get_string_param(params, "plan_id")?;
            let report = store
                .coverage_report(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.reconcile" => {
            let id = get_string_param(params, "plan_id")?;
            let fix = p.and_then(|x| x.get("fix")).and_then(|v| v.as_bool()) == Some(true);
            let caller = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let report = svc
                .reconcile_plan(&id, fix, caller, now)
                .map_err(plan_rpc_err)?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.groups.set" => {
            let id = get_string_param(params, "plan_id")?;
            let groups = get_string_param(params, "groups")?;
            store
                .set_task_groups(&id, groups_from_csv(&groups), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.link" => {
            let id = get_string_param(params, "plan_id")?;
            let to = get_string_param(params, "to")?;
            store.link_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.unlink" => {
            let id = get_string_param(params, "plan_id")?;
            let to = get_string_param(params, "to")?;
            store.unlink_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.task.add" => {
            let id = get_string_param(params, "plan_id")?;
            let name = get_string_param(params, "name")?;
            let kind = p
                .and_then(|x| x.get("type"))
                .or_else(|| p.and_then(|x| x.get("kind")))
                .and_then(|v| v.as_str())
                .unwrap_or("implement")
                .to_string();
            let requested_sections = csv_string_field(p, "sections");
            let section_ids = store
                .resolve_task_section_ids(&id, &requested_sections)
                .map_err(plan_rpc_err)?;
            let has_sections = !section_ids.is_empty();
            let has_model = p
                .and_then(|x| x.get("model"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let has_why = p
                .and_then(|x| x.get("why"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let acceptance_criteria = string_array_field(p, "acceptance_criteria");
            let has_criteria = acceptance_criteria.iter().any(|s| !s.trim().is_empty());
            let test_plan = p
                .and_then(|x| x.get("test_plan"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let correctness = p
                .and_then(|x| x.get("correctness"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let tid = store
                .add_task(
                    &id,
                    NewTask {
                        name,
                        kind,
                        section_ids,
                        model: p
                            .and_then(|x| x.get("model"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        model_rationale: p
                            .and_then(|x| x.get("why"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        acceptance_criteria,
                        test_plan,
                        correctness,
                        body: p
                            .and_then(|x| x.get("body"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        intent: p
                            .and_then(|x| x.get("intent"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        parent_task_id: p
                            .and_then(|x| x.get("parent"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        assigned_pane_id: p
                            .and_then(|x| x.get("assign_pane"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        lane: lane_metadata_field(p)?,
                        group: p
                            .and_then(|x| x.get("group"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        priority: p
                            .and_then(|x| x.get("priority"))
                            .and_then(|v| v.as_str())
                            .map(|s| serde_json::from_value(serde_json::Value::String(s.into())))
                            .transpose()
                            .map_err(|_| plan_rpc_err(PlanError::BadPriority))?,
                        handle: p
                            .and_then(|x| x.get("handle"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        tags: tags_field(p),
                    },
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let linked = store
                .get(&id)
                .and_then(|plan| {
                    plan.tasks
                        .iter()
                        .find(|task| task.id == tid)
                        .map(|task| task.section_ids.clone())
                })
                .unwrap_or_default();
            let linked_has_sections = !linked.is_empty();
            let mut response_hints = hints::task_create_hints(
                &id,
                &tid,
                has_sections || linked_has_sections,
                has_model,
                has_why,
                has_criteria,
                has_test_plan,
                has_correctness,
            );
            if let Some(hint) = hints::maybe_orchestration_reminder() {
                response_hints.push(hint);
            }
            Ok(json!({
                "task_id": tid.clone(),
                "section_ids": linked,
                "_hints": response_hints
            }))
        }

        "plan.task.update" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let sections = p.and_then(|x| x.get("sections")).map(|_| {
                store
                    .resolve_task_section_ids(&id, &csv_string_field(p, "sections"))
                    .map_err(plan_rpc_err)
            });
            let sections = match sections {
                Some(result) => Some(result?),
                None => None,
            };
            let acceptance_criteria = p
                .and_then(|x| x.get("acceptance_criteria"))
                .map(|_| string_array_field(p, "acceptance_criteria"));
            let test_plan = optional_string_field(p, "test_plan");
            let correctness = optional_string_field(p, "correctness");
            let lane = lane_update_field(p)?;
            store
                .update_task(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("name")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("type"))
                        .or_else(|| p.and_then(|x| x.get("kind")))
                        .and_then(|v| v.as_str()),
                    optional_string_field(p, "model"),
                    optional_string_field(p, "why"),
                    p.and_then(|x| x.get("status")).and_then(|v| v.as_str()),
                    optional_string_field(p, "status_label"),
                    p.and_then(|x| x.get("priority")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    optional_string_field(p, "intent"),
                    sections,
                    p.and_then(|x| x.get("parent_task_id").or_else(|| x.get("parent")))
                        .map(|v| v.as_str()),
                    optional_string_field(p, "group"),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(lane) = lane {
                store
                    .update_task_lane(&id, &tid, lane, now)
                    .map_err(plan_rpc_err)?;
            }
            if acceptance_criteria.is_some() || test_plan.is_some() || correctness.is_some() {
                store
                    .update_task_quality(
                        &id,
                        &tid,
                        acceptance_criteria,
                        test_plan,
                        correctness,
                        now,
                    )
                    .map_err(plan_rpc_err)?;
            }
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.task.link" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let sections = csv_string_field(p, "sections");
            let linked = store
                .link_task_sections(&id, &tid, &sections, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "section_ids": linked }))
        }

        "plan.task.relink" => {
            let id = get_string_param(params, "plan_id")?;
            let apply = p.and_then(|x| x.get("apply")).and_then(|v| v.as_bool()) == Some(true);
            let report = store
                .relink_unlinked_tasks(&id, apply, now)
                .map_err(plan_rpc_err)?;
            if apply && !report.applied.is_empty() {
                emit_plan_changed(app, &id);
            }
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.task.close" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| AgentRef::from_pane("unknown"));
            store
                .close_task(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("result")).and_then(|v| v.as_str()),
                    from,
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.task.assign" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let pane = get_string_param(params, "pane")?;
            store
                .assign_task(
                    &id,
                    &tid,
                    &pane,
                    p.and_then(|x| x.get("peer")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.task.rm" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            store.rm_task(&id, &tid, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.milestone.add" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let kind = get_string_param(params, "kind")?;
            let text = p
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let add_kind = p
                .and_then(|x| x.get("add_kind"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| AgentRef::from_pane("unknown"));
            let mid = store
                .add_milestone(&id, &tid, &kind, text, from, add_kind, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "milestone_id": mid }))
        }

        "plan.milestone.update" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let ms_id = get_string_param(params, "milestone_id")?;
            store
                .update_milestone(
                    &id,
                    &tid,
                    &ms_id,
                    p.and_then(|x| x.get("kind")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("text")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.milestone.rm" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let ms_id = get_string_param(params, "milestone_id")?;
            store
                .rm_milestone(&id, &tid, &ms_id, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.loop.create" => {
            let id = get_string_param(params, "plan_id")?;
            let target = get_string_param(params, "target")?;
            let budget = p
                .and_then(|x| x.get("budget"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            let stages = p
                .and_then(|x| x.get("stages"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let lid = store
                .create_loop(
                    &id,
                    &target,
                    stages,
                    budget,
                    p.and_then(|x| x.get("stop_when")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "loop_id": lid }))
        }

        "plan.loop.set" => {
            let lid = get_string_param(params, "loop_id")?;
            store
                .set_loop(
                    &lid,
                    p.and_then(|x| x.get("budget"))
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32),
                    Some(p.and_then(|x| x.get("stop_when")).and_then(|v| v.as_str())),
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(plan_id) = store.list_index().iter().find_map(|e| {
                store
                    .get(&e.plan_id)
                    .filter(|pl| pl.loops.iter().any(|l| l.id == lid))
                    .map(|pl| pl.id)
            }) {
                emit_plan_changed(app, &plan_id);
            }
            Ok(json!({ "ok": true }))
        }

        "plan.loop.status" => {
            let lid = get_string_param(params, "loop_id")?;
            let spec = store
                .loop_status(&lid)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(spec).unwrap_or_default())
        }

        "plan.loop.stop" => {
            let lid = get_string_param(params, "loop_id")?;
            store.stop_loop(&lid, now).map_err(plan_rpc_err)?;
            if let Some(plan_id) = store.list_index().iter().find_map(|e| {
                store
                    .get(&e.plan_id)
                    .filter(|pl| pl.loops.iter().any(|l| l.id == lid))
                    .map(|pl| pl.id)
            }) {
                emit_plan_changed(app, &plan_id);
            }
            Ok(json!({ "ok": true }))
        }

        "plan.layout.set" => {
            let id = get_string_param(params, "plan_id")?;
            let element = get_string_param(params, "element")?;
            store
                .set_layout(
                    &id,
                    &element,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.note.add" => {
            let id = get_string_param(params, "plan_id")?;
            let text = p
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let links = p
                .and_then(|x| x.get("links"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let eid = store
                .add_note(
                    &id,
                    text,
                    links,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "element_id": eid }))
        }

        "plan.element.add" => {
            let id = get_string_param(params, "plan_id")?;
            let kind = get_string_param(params, "kind")?;
            let content = p
                .and_then(|x| x.get("content"))
                .cloned()
                .unwrap_or(json!({}));
            let links = p
                .and_then(|x| x.get("links"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let eid = store
                .add_element(
                    &id,
                    &kind,
                    content,
                    links,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "element_id": eid }))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests;
