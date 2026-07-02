use std::path::Path;

use serde_json::{json, Value};

use crate::service::plan::search_index::PlanSearchStatus;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::plan_linking::plan_with_linking;
use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.get"
            | "plan.spec"
            | "plan.files"
            | "plan.tags"
            | "plan.related"
            | "plan.search"
            | "plan.reindex"
    )
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    match method {
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
        "plan.related" => {
            let id = get_string_param(params, "id")?;
            let depth = params
                .as_ref()
                .and_then(|x| x.get("depth"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            Ok(json!({
                "id": id,
                "depth": depth,
                "groups": store.related_units(&id),
            }))
        }
        "plan.search" => {
            let query = get_string_param(params, "q")?;
            let project = params
                .as_ref()
                .and_then(|x| x.get("project"))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let kind = params
                .as_ref()
                .and_then(|x| x.get("kind"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let limit = params
                .as_ref()
                .and_then(|x| x.get("limit"))
                .and_then(|v| v.as_u64())
                .and_then(|v| usize::try_from(v).ok());
            let status = match params
                .as_ref()
                .and_then(|x| x.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("active")
            {
                "active" => PlanSearchStatus::Active,
                "archived" => PlanSearchStatus::Archived,
                "all" => PlanSearchStatus::All,
                other => {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: format!("invalid plan search status: {other}"),
                        data: None,
                    });
                }
            };
            let project_root = svc
                .ensure_plan_project_loaded(Path::new(project))
                .map_err(plan_rpc_err)?;
            let hits = store
                .search_plan_index_with_status(
                    &project_root,
                    &crate::mem::search::SearchQuery {
                        query,
                        kind,
                        limit,
                        ..Default::default()
                    },
                    status,
                )
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            Ok(json!({ "hits": hits }))
        }
        "plan.reindex" => {
            let project = get_string_param(params, "project")?;
            let project_root = svc
                .ensure_plan_project_loaded(Path::new(&project))
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
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
