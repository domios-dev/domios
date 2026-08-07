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
        code: e.rpc_code(),
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

fn bad_param(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message,
        data: None,
    }
}

/// Coverage edges for `spec.link.add`: an `edges` array of
/// `{spec_section_id, plan_section_id}` objects, else the legacy single pair.
fn spec_link_edges(params: &Option<Value>) -> Result<Vec<(String, String)>, JsonRpcError> {
    if let Some(items) = params
        .as_ref()
        .and_then(|x| x.get("edges"))
        .and_then(|v| v.as_array())
    {
        let mut edges = Vec::with_capacity(items.len());
        for item in items {
            let ss = item
                .get("spec_section_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let ps = item
                .get("plan_section_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match (ss, ps) {
                (Some(ss), Some(ps)) => edges.push((ss.to_string(), ps.to_string())),
                _ => {
                    return Err(bad_param(
                        "each edge needs non-empty spec_section_id and plan_section_id".into(),
                    ))
                }
            }
        }
        if edges.is_empty() {
            return Err(bad_param("edges must not be empty".into()));
        }
        return Ok(edges);
    }
    let spec_section_id = get_string_param(params, "spec_section_id")?;
    let plan_section_id = get_string_param(params, "plan_section_id")?;
    Ok(vec![(spec_section_id, plan_section_id)])
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
        "spec.delete" => {
            let id = super::plan_refs::resolve_spec(store, params)?;
            store.delete_spec(&id, now).map_err(plan_rpc_err)?;
            emit_spec_changed(app, &id);
            Ok(json!({ "ok": true, "spec_id": id }))
        }
        "spec.show" => {
            let id = super::plan_refs::resolve_spec(store, params)?;
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
        "spec.section.add" => {
            let id = super::plan_refs::resolve_spec(store, params)?;
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
            let (id, sid) = super::plan_refs::resolve_spec_section(store, params)?;
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
            let (id, sid) = super::plan_refs::resolve_spec_section(store, params)?;
            store
                .rm_spec_section(&id, &sid, now)
                .map_err(plan_rpc_err)?;
            emit_spec_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "spec.section.show" => {
            let (id, sid) = super::plan_refs::resolve_spec_section(store, params)?;
            let result = store.show_spec_section(&id, &sid).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "spec.link.add" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let spec_id = super::plan_refs::resolve_spec(store, params)?;
            let edges = spec_link_edges(params)?;
            let mut results = Vec::with_capacity(edges.len());
            let mut all_ok = true;
            for (spec_section_id, plan_section_id) in edges {
                match store.link_spec_section(
                    &plan_id,
                    &spec_id,
                    &spec_section_id,
                    &plan_section_id,
                    now,
                ) {
                    Ok(()) => results.push(json!({
                        "spec_section_id": spec_section_id,
                        "plan_section_id": plan_section_id,
                        "ok": true,
                    })),
                    Err(e) => {
                        all_ok = false;
                        results.push(json!({
                            "spec_section_id": spec_section_id,
                            "plan_section_id": plan_section_id,
                            "ok": false,
                            "error": e.to_string(),
                        }));
                    }
                }
            }
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "ok": all_ok, "results": results }))
        }
        "spec.link.rm" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let spec_id = super::plan_refs::resolve_spec(store, params)?;
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
            let spec_id = super::plan_refs::resolve_spec(store, params)?;
            Ok(json!({ "links": store.list_spec_links(&spec_id) }))
        }
        "effort.show" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
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

#[cfg(test)]
mod tests {
    use super::spec_link_edges;
    use serde_json::json;

    #[test]
    fn edges_array_parses_all_pairs() {
        let params = Some(json!({
            "edges": [
                { "spec_section_id": "s1", "plan_section_id": "p1" },
                { "spec_section_id": "s2", "plan_section_id": "p2" },
            ]
        }));
        let edges = spec_link_edges(&params).ok();
        assert_eq!(
            edges,
            Some(vec![
                ("s1".to_string(), "p1".to_string()),
                ("s2".to_string(), "p2".to_string())
            ])
        );
    }

    #[test]
    fn legacy_single_pair_still_supported() {
        let params = Some(json!({ "spec_section_id": "s1", "plan_section_id": "p1" }));
        let edges = spec_link_edges(&params).ok();
        assert_eq!(edges, Some(vec![("s1".to_string(), "p1".to_string())]));
    }

    #[test]
    fn edge_with_empty_side_is_rejected() {
        let params = Some(json!({
            "edges": [ { "spec_section_id": "s1", "plan_section_id": "" } ]
        }));
        assert!(spec_link_edges(&params).is_err());
    }
}
