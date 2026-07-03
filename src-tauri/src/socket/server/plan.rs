use super::plan_fields::{
    groups_from_csv, kinds_from_csv, optional_string_field, pane_and_ancestors, pane_role,
    string_array_field, tag_facets_field,
};
use super::plan_lane_fields::{lane_metadata_field, lane_update_field};
use super::plan_linking::{csv_string_field, owner_from_params, tags_field};
use super::plan_refs;
use super::plan_role_helpers::{assign_task_with_profile, RespawnAttemptRoleExt};
use super::{get_string_param, JsonRpcError};
use crate::plan::{OrchestratorRef, PlanStatus, PlannerRef, TaskStatus};
use crate::service::plan::batch::BatchRequest;
use crate::service::plan::task_queries::TaskListFilter;
use crate::service::plan::{hints, NewTask, PlanError};
use crate::service::ServiceState;
use serde_json::{json, Value};
use tauri::AppHandle;
pub(super) fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    let data = e.to_json();
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(data),
    }
}
pub(super) fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
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
    if method.starts_with("plan.qa.") {
        return super::plan_qa::dispatch(method, params, app, svc);
    }
    if super::plan_read::handles(method) {
        return super::plan_read::dispatch(method, params, svc);
    }
    if super::plan_elements::handles(method) {
        return super::plan_elements::dispatch(method, params, app, svc);
    }
    if super::plan_loops::handles(method) {
        return super::plan_loops::dispatch(method, params, app, svc);
    }
    if super::plan_lifecycle::handles(method) {
        return super::plan_lifecycle::dispatch(method, params, app, svc);
    }
    if super::plan_attachments::handles(method) {
        return super::plan_attachments::dispatch(method, params, app, svc);
    }
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = super::plan_time::epoch_ms();

    match method {
        "plan.batch" => {
            let id = plan_refs::resolve_plan(store, params)?;
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

        "task.list" => {
            let caller = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mine = p.and_then(|x| x.get("mine")).and_then(|v| v.as_bool()) == Some(true);
            let require_plan_id = p
                .and_then(|x| x.get("require_plan_id"))
                .and_then(|v| v.as_bool())
                == Some(true);
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|status| {
                    serde_json::from_value::<TaskStatus>(Value::String(status.into()))
                        .map_err(|_| plan_rpc_err(PlanError::BadStatus))
                })
                .transpose()?;
            let plan_id = if require_plan_id {
                Some(plan_refs::resolve_plan(store, params)?)
            } else {
                p.and_then(|x| x.get("plan_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
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
                plan_id,
                status,
            });
            Ok(json!({ "tasks": tasks }))
        }

        "task.show" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
            let detail = store
                .show_task(&task_id)
                .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
            Ok(serde_json::to_value(detail).unwrap_or_default())
        }

        "task.take" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
            let pane_id = get_string_param(params, "caller_pane_id")?;
            let role = pane_role(svc, &pane_id);
            let plan_id = store
                .set_task_assignment(&task_id, Some(&pane_id), role, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": true, "plan_id": plan_id, "task_id": task_id }))
        }

        "task.drop" => {
            let (_pid, task_id) = plan_refs::resolve_task(store, params)?;
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
                        competition: super::plan_competition::metadata_field(p)?,
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

        "plan.update" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let status = p
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .map(|s| serde_json::from_value::<PlanStatus>(Value::String(s.into())))
                .transpose()
                .map_err(|_| plan_rpc_err(PlanError::BadStatus))?;
            let mode = p.and_then(|x| x.get("mode")).and_then(|v| v.as_str());
            store
                .update_plan(
                    &id,
                    p.and_then(|x| x.get("title")).and_then(|v| v.as_str()),
                    status,
                    mode,
                    now,
                )
                .map_err(plan_rpc_err)?;
            if let Some(planner_val) = p.and_then(|x| x.get("planner")) {
                let planner_ref: Option<PlannerRef> = serde_json::from_value(planner_val.clone())
                    .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid planner: {e}"),
                    data: None,
                })?;
                store
                    .set_planner_ref(&id, planner_ref, now)
                    .map_err(plan_rpc_err)?;
            }
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "plan.archive" => super::plan_archive::archive(store, svc, app, params, now),
        "plan.unarchive" => super::plan_archive::unarchive(store, app, params, now),
        "plan.delete" => {
            let id = plan_refs::resolve_plan(store, params)?;
            store.delete(&id).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.transfer" => {
            let id = plan_refs::resolve_plan(store, params)?;
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
            let id = plan_refs::resolve_plan(store, params)?;
            store.disown(&id, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.claim" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let params_val = p.cloned().unwrap_or(json!({}));
            let owner: OrchestratorRef = params_val
                .get("owner")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| owner_from_params(&params_val));
            store.claim(&id, owner.clone(), now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            let mut response_hints = Vec::new();
            if matches!(
                store.resolve_planner_pane(&id, |pid| svc.pane_probe(pid)),
                crate::plan::peer::PeerResolution::Unbound
            ) {
                let spawn_cmd = crate::service::plan::peer_mandate::planner_spawn_cmd_hint(
                    owner.pane_id.as_deref(),
                    &id,
                );
                response_hints.push(crate::prompts::peer_pairing_mandate_full(&spawn_cmd));
            }
            Ok(json!({ "ok": true, "_hints": response_hints }))
        }

        "plan.ingest" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let markdown = get_string_param(params, "markdown")?;
            let result = store
                .ingest_plan_sections(&id, &markdown, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(serde_json::to_value(result).unwrap_or_else(|_| json!({ "sections": [] })))
        }

        "plan.section.add" => {
            let id = plan_refs::resolve_plan(store, params)?;
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
            let caller_pane = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "section_id": sid });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_pane))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.section.update" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
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
            let caller_pane = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "ok": true });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_pane))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.section.rm" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            store.rm_section(&id, &sid, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.section.show" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            let result = store.show_section(&id, &sid).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "plan.kinds.set" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let kinds = get_string_param(params, "kinds")?;
            store
                .set_kinds(&id, kinds_from_csv(&kinds), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.tag_facets.set" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let facets =
                tag_facets_field(params.as_ref(), "facets").map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            store
                .set_tag_facets(&id, facets, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.coverage" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let report = store
                .coverage_report(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.state" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let snapshot = svc.build_plan_state(&id, now).map_err(plan_rpc_err)?;
            Ok(serde_json::to_value(snapshot).unwrap_or_default())
        }

        "plan.next" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let snapshot = svc.build_plan_state(&id, now).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": id, "next_actions": snapshot.next_actions }))
        }

        "plan.reconcile" => {
            let id = plan_refs::resolve_plan(store, params)?;
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
            let id = plan_refs::resolve_plan(store, params)?;
            let groups = get_string_param(params, "groups")?;
            store
                .set_task_groups(&id, groups_from_csv(&groups), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.link" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let to = get_string_param(params, "to")?;
            store.link_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.unlink" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let to = get_string_param(params, "to")?;
            store.unlink_plan(&id, &to, now).map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }

        "plan.task.add" => {
            let id = plan_refs::resolve_plan(store, params)?;
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
            let force = p.and_then(|x| x.get("force")).and_then(|v| v.as_bool()) == Some(true);
            store
                .ensure_task_creation_allowed(&id, &kind, force)
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
                        role_guidance: p
                            .and_then(|x| x.get("role_guidance"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        executor_role: p
                            .and_then(|x| x.get("executor_role"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        competition: super::plan_competition::metadata_field(p)?,
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
            let caller_pane = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({
                "task_id": tid.clone(),
                "section_ids": linked,
                "_hints": response_hints
            });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_pane))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.task.update" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
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
                    optional_string_field(p, "executor_role"),
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
            super::plan_role_helpers::update_role_guidance_if_present(store, &id, &tid, p, now)?;
            if let Some(lane) = lane {
                store
                    .update_task_lane(&id, &tid, lane, now)
                    .map_err(plan_rpc_err)?;
            }
            if let Some(competition) = super::plan_competition::update_field(p)? {
                store
                    .update_task_competition(&id, &tid, competition, now)
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
            let caller_pane = p
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str());
            let mut resp = json!({ "ok": true });
            if let Some(nudge) = store
                .get(&id)
                .as_ref()
                .and_then(|plan| hints::planner_ownership_nudge(plan, caller_pane))
            {
                resp["_planner_nudge"] = json!(nudge);
            }
            Ok(resp)
        }

        "plan.task.link" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let sections = csv_string_field(p, "sections");
            let linked = store
                .link_task_sections(&id, &tid, &sections, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "section_ids": linked }))
        }

        "plan.task.relink" => {
            let id = plan_refs::resolve_plan(store, params)?;
            let apply = p.and_then(|x| x.get("apply")).and_then(|v| v.as_bool()) == Some(true);
            let report = store
                .relink_unlinked_tasks(&id, apply, now)
                .map_err(plan_rpc_err)?;
            if apply && !report.applied.is_empty() {
                emit_plan_changed(app, &id);
            }
            Ok(serde_json::to_value(report).unwrap_or_default())
        }

        "plan.task.assign" => {
            let id = assign_task_with_profile(store, svc, params, p, now)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "plan.task.attempts" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let attempts = store.list_task_attempts(&id, &tid).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": id, "task_id": tid, "attempts": attempts }))
        }
        "plan.task.attempt.show" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            let attempt = store
                .show_task_attempt(&id, &tid, &attempt_id)
                .map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": id, "task_id": tid, "attempt": attempt }))
        }

        "plan.task.respawn" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request = crate::service::plan::task_attempts::TaskRespawnRequest {
                attempt_id: get_string_param(params, "attempt_id")?,
                provider: body
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                model: body
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                parent_pane_id: body
                    .get("parent_pane_id")
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
            let draft = store
                .build_task_respawn_draft(&id, &tid, request)
                .map_err(plan_rpc_err)?;
            let spawn_params = crate::service::SpawnAgentParams {
                agent: Some(draft.provider.clone()),
                cwd: Some(draft.cwd.clone()),
                title: Some(draft.title.clone()),
                role: Some("Worker".into()),
                parent_pane_id: draft.parent_pane_id.clone(),
                task: Some(draft.task_prompt.clone()),
                workspace_id: draft.workspace_id.clone(),
                model: draft.model.clone(),
                spawned_by_pane_id: draft.parent_pane_id.clone(),
                ..Default::default()
            };
            let spawn = svc
                .spawn_agent_sync(spawn_params)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            let created = store
                .record_task_respawn_attempt_with_profile(
                    &id,
                    &tid,
                    &spawn.pane_id,
                    &draft,
                    svc,
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({
                "ok": true,
                "plan_id": id,
                "task_id": tid,
                "pane_id": spawn.pane_id,
                "spawn": spawn,
                "attempt": created.attempt,
                "draft": draft
            }))
        }

        "plan.task.compete" => super::plan_competition::compete(params, app, svc, now),

        "plan.task.judge" => super::plan_competition::judge(params, app, svc, now),

        "plan.task.rm" => {
            let all = p.and_then(|x| x.get("all")).and_then(|v| v.as_bool()) == Some(true);
            let (id, count) = if all {
                let id = plan_refs::resolve_plan(store, params)?;
                let count = store.rm_all_tasks(&id, now).map_err(plan_rpc_err)?;
                (id, count)
            } else {
                let (id, tid) = plan_refs::resolve_task(store, params)?;
                store.rm_task(&id, &tid, now).map_err(plan_rpc_err)?;
                (id, 1)
            };
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "removed": count }))
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
