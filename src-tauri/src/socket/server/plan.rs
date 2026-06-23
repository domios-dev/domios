//! `plan.*` JSON-RPC handlers.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{AgentRef, MilestoneKind, OrchestratorRef, PlanStatus, TaskGroup};
use crate::service::plan::{NewTask, PlanError};
use crate::service::ServiceState;

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

fn kinds_from_csv(raw: &str) -> Vec<MilestoneKind> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|key| MilestoneKind {
            key: key.into(),
            label: key.into(),
            color: None,
        })
        .collect()
}

fn groups_from_csv(raw: &str) -> Vec<TaskGroup> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (key, label) = item.split_once(':').unwrap_or((item, item));
            TaskGroup {
                key: key.trim().into(),
                label: label.trim().into(),
                color: None,
            }
        })
        .collect()
}

fn owner_from_params(p: &Value) -> OrchestratorRef {
    p.get("owner")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .or_else(|| {
            p.get("pane_id")
                .and_then(|v| v.as_str())
                .map(OrchestratorRef::from_pane)
        })
        .unwrap_or_else(|| OrchestratorRef::from_pane("unknown"))
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = epoch_ms();

    match method {
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
            let plan = store
                .create_plan(title, project, owner_from_params(body), kinds, now)
                .map_err(plan_rpc_err)?;
            if let Some(spec) = body
                .get("spec")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                let _ = store.add_section(&plan.id, "Spec", spec, None, now);
            }
            emit_plan_changed(app, &plan.id);
            Ok(json!({ "plan_id": plan.id, "plan": plan }))
        }

        "plan.list" => {
            let mut entries = store.list_index();
            if let Some(project) = p.and_then(|x| x.get("project")).and_then(|v| v.as_str()) {
                let want = crate::service::plan::paths::project_root(std::path::Path::new(project))
                    .display()
                    .to_string();
                entries.retain(|e| {
                    store
                        .get(&e.plan_id)
                        .map(|pl| pl.project_path == want)
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

        "plan.get" => {
            let id = get_string_param(params, "plan_id")?;
            let plan = store
                .get(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(plan).unwrap_or_default())
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

        "plan.section.add" => {
            let id = get_string_param(params, "plan_id")?;
            let title = get_string_param(params, "title")?;
            let body = p
                .and_then(|x| x.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let parent = p.and_then(|x| x.get("parent")).and_then(|v| v.as_str());
            let sid = store
                .add_section(&id, &title, body, parent, now)
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
                    p.and_then(|x| x.get("title")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
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
            let section_ids = p
                .and_then(|x| x.get("sections"))
                .and_then(|v| v.as_str())
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(|x| x.to_string())
                        .collect()
                })
                .unwrap_or_default();
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
                        body: p
                            .and_then(|x| x.get("body"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        intent: p
                            .and_then(|x| x.get("intent"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        assigned_pane_id: p
                            .and_then(|x| x.get("assign_pane"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        group: p
                            .and_then(|x| x.get("group"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    },
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "task_id": tid }))
        }

        "plan.task.update" => {
            let id = get_string_param(params, "plan_id")?;
            let tid = get_string_param(params, "task_id")?;
            let sections = p
                .and_then(|x| x.get("sections"))
                .and_then(|v| v.as_str())
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(|x| x.to_string())
                        .collect()
                });
            store
                .update_task(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("name")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("type"))
                        .or_else(|| p.and_then(|x| x.get("kind")))
                        .and_then(|v| v.as_str()),
                    Some(p.and_then(|x| x.get("model")).and_then(|v| v.as_str())),
                    Some(p.and_then(|x| x.get("why")).and_then(|v| v.as_str())),
                    p.and_then(|x| x.get("status")).and_then(|v| v.as_str()),
                    Some(
                        p.and_then(|x| x.get("status_label"))
                            .and_then(|v| v.as_str()),
                    ),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    Some(p.and_then(|x| x.get("intent")).and_then(|v| v.as_str())),
                    sections,
                    p.and_then(|x| x.get("group"))
                        .and_then(|v| v.as_str())
                        .map(Some),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
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
