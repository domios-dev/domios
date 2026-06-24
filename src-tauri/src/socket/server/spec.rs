use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{EffortLinkRel, PlanStatus};
use crate::service::plan::PlanError;
use crate::service::ServiceState;

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

fn emit_spec_changed(app: &AppHandle, spec_id: &str) {
    crate::api::events::emit_everywhere(app, "spec-changed", json!({ "spec_id": spec_id }));
}

fn tags_field(params: Option<&Value>) -> Vec<String> {
    params
        .and_then(|x| x.get("tags"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
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
        "spec.create" => {
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let title = get_string_param(params, "title")?;
            let project = body
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                });
            let spec = store
                .create_spec(title, project, now)
                .map_err(plan_rpc_err)?;
            emit_spec_changed(app, &spec.id);
            Ok(json!({ "spec_id": spec.id, "spec": spec }))
        }
        "spec.list" => {
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
            let mut entries = store.list_spec_index();
            if let Some(want) = &want {
                entries.retain(|entry| entry.project_path == *want);
            }
            if let Some(status) = p.and_then(|x| x.get("status")).and_then(|v| v.as_str()) {
                entries.retain(|e| match status {
                    "active" => e.status == PlanStatus::Active,
                    "archived" => e.status == PlanStatus::Archived,
                    _ => true,
                });
            }
            Ok(json!({ "specs": entries }))
        }
        "spec.show" => {
            let id = get_string_param(params, "spec_id")?;
            if p.and_then(|x| x.get("full")).and_then(|v| v.as_bool()) == Some(true) {
                return store
                    .spec_full(&id)
                    .ok_or_else(|| plan_rpc_err(PlanError::NotFound));
            }
            let spec = store
                .get_spec(&id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(json!({ "spec": spec }))
        }
        "spec.files" => {
            let id = get_string_param(params, "spec_id")?;
            let manifest = store.spec_file_manifest(&id).map_err(plan_rpc_err)?;
            Ok(serde_json::to_value(manifest).unwrap_or_default())
        }
        "spec.section.add" => {
            let id = get_string_param(params, "spec_id")?;
            let title = get_string_param(params, "title")?;
            let body = p
                .and_then(|x| x.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sid = store
                .add_spec_section(
                    &id,
                    &title,
                    body,
                    p.and_then(|x| x.get("parent")).and_then(|v| v.as_str()),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_spec_changed(app, &id);
            Ok(json!({ "section_id": sid }))
        }
        "spec.section.update" => {
            let id = get_string_param(params, "spec_id")?;
            let sid = get_string_param(params, "section_id")?;
            store
                .update_spec_section(
                    &id,
                    &sid,
                    p.and_then(|x| x.get("title")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("body")).and_then(|v| v.as_str()),
                    &tags_field(p),
                    p.and_then(|x| x.get("primary")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("handle")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_spec_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "spec.section.rm" => {
            let id = get_string_param(params, "spec_id")?;
            let sid = get_string_param(params, "section_id")?;
            store
                .rm_spec_section(&id, &sid, now)
                .map_err(plan_rpc_err)?;
            emit_spec_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "spec.link.add" => {
            let plan_id = get_string_param(params, "plan_id")?;
            let spec_id = get_string_param(params, "spec_id")?;
            let spec_section_id = get_string_param(params, "spec_section_id")?;
            let plan_section_id = get_string_param(params, "plan_section_id")?;
            store
                .link_spec_section(&plan_id, &spec_id, &spec_section_id, &plan_section_id, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": true }))
        }
        "spec.link.rm" => {
            let plan_id = get_string_param(params, "plan_id")?;
            let spec_id = get_string_param(params, "spec_id")?;
            store
                .unlink_spec_section(
                    &plan_id,
                    &spec_id,
                    p.and_then(|x| x.get("spec_section_id"))
                        .and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("plan_section_id"))
                        .and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": true }))
        }
        "spec.link.list" => {
            let spec_id = get_string_param(params, "spec_id")?;
            Ok(json!({ "links": store.list_spec_links(&spec_id) }))
        }
        "effort.show" => {
            let plan_id = get_string_param(params, "plan_id")?;
            store
                .effort_bundle(&plan_id)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))
        }
        "effort.link" => {
            let from = get_string_param(params, "from")?;
            let to = get_string_param(params, "to")?;
            let rel_raw = get_string_param(params, "rel")?;
            let rel = match rel_raw.as_str() {
                "follows" => EffortLinkRel::Follows,
                "supersedes" => EffortLinkRel::Supersedes,
                _ => {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: "rel must be follows or supersedes".into(),
                        data: None,
                    });
                }
            };
            store
                .link_effort(&from, &to, rel, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &from);
            Ok(json!({ "ok": true }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
