use codeterm_service_core::service::plan::radar::{
    NewRadarItem, RadarEdit, RadarError, RadarFilter, RadarPullRequest, RadarSource, RadarStatus,
};
use codeterm_service_core::service::ServiceState;
use serde_json::{json, Value};

use super::error_codes::{INVALID_PARAMS, NOT_FOUND, PLAN_ORCHESTRATOR_REQUIRED};
use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_linking::{csv_string_field, tags_field};
use super::plan_task_rpc::authenticated_caller;
use super::{get_string_param, plan_refs, JsonRpcError};

fn radar_rpc_err(error: RadarError) -> JsonRpcError {
    let message = error.to_string();
    match error {
        RadarError::Plan(error) => plan_rpc_err(error),
        RadarError::NotFound(_) => JsonRpcError {
            code: NOT_FOUND,
            message,
            data: None,
        },
        RadarError::AlreadyPulled(pull) => JsonRpcError {
            code: INVALID_PARAMS,
            message,
            data: Some(json!({ "plan_id": pull.plan_id, "task_id": pull.task_id })),
        },
        RadarError::Invalid(_) => JsonRpcError {
            code: INVALID_PARAMS,
            message,
            data: None,
        },
        RadarError::Io(_) => JsonRpcError {
            code: -32603,
            message,
            data: None,
        },
    }
}

fn invalid(message: String) -> JsonRpcError {
    JsonRpcError {
        code: INVALID_PARAMS,
        message,
        data: None,
    }
}

fn optional_string(params: Option<&Value>, key: &str) -> Option<String> {
    params
        .and_then(|p| p.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn optional_tags(params: Option<&Value>) -> Option<Vec<String>> {
    params
        .and_then(|p| p.get("tags"))
        .is_some()
        .then(|| tags_field(params))
}

/// `open` unless asked otherwise; `all` lists every status.
pub(super) fn status_filter(raw: Option<&str>) -> Result<Vec<RadarStatus>, JsonRpcError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(vec![RadarStatus::Open]),
        Some("all") => Ok(Vec::new()),
        Some(other) => RadarStatus::parse(other)
            .map(|status| vec![status])
            .ok_or_else(|| {
                invalid(format!(
                    "unknown radar status '{other}'; use open, pulled or all"
                ))
            }),
    }
}

pub(super) fn source_field(params: Option<&Value>) -> Result<Option<RadarSource>, JsonRpcError> {
    let provider = optional_string(params, "source_provider");
    let external_id = optional_string(params, "source_id");
    let url = optional_string(params, "source_url");
    match (provider, external_id) {
        (Some(provider), Some(external_id)) => Ok(Some(RadarSource {
            provider,
            external_id,
            url,
        })),
        (None, None) if url.is_none() => Ok(None),
        _ => Err(invalid(
            "a radar source needs both source_provider and source_id".into(),
        )),
    }
}

fn item_json(item: impl serde::Serialize) -> Result<Value, JsonRpcError> {
    serde_json::to_value(item).map_err(|error| JsonRpcError {
        code: -32603,
        message: error.to_string(),
        data: None,
    })
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = super::plan_time::epoch_ms();
    match method {
        "radar.add" => {
            let added = store
                .radar()
                .add(
                    NewRadarItem {
                        project_path: get_string_param(params, "project")?,
                        title: get_string_param(params, "title")?,
                        body: optional_string(p, "body").unwrap_or_default(),
                        tags: tags_field(p),
                        source: source_field(p)?,
                    },
                    now,
                )
                .map_err(radar_rpc_err)?;
            Ok(json!({ "item": item_json(&added.item)?, "created": added.created }))
        }
        "radar.list" => {
            let filter = RadarFilter {
                project_path: optional_string(p, "project")
                    .filter(|project| !project.trim().is_empty()),
                statuses: status_filter(optional_string(p, "status").as_deref())?,
                tag: optional_string(p, "tag"),
                provider: optional_string(p, "provider"),
                query: optional_string(p, "query"),
            };
            let items = store.radar().list(&filter).map_err(radar_rpc_err)?;
            Ok(json!({ "items": item_json(&items)? }))
        }
        "radar.show" => {
            let item = store
                .radar()
                .get(&get_string_param(params, "item_id")?)
                .map_err(radar_rpc_err)?;
            Ok(json!({ "item": item_json(&item)? }))
        }
        "radar.update" => {
            let item = store
                .radar()
                .update(
                    &get_string_param(params, "item_id")?,
                    RadarEdit {
                        title: optional_string(p, "title"),
                        body: optional_string(p, "body"),
                        tags: optional_tags(p),
                    },
                    now,
                )
                .map_err(radar_rpc_err)?;
            Ok(json!({ "item": item_json(&item)? }))
        }
        "radar.drop" => {
            let item = store
                .radar()
                .drop_item(&get_string_param(params, "item_id")?)
                .map_err(radar_rpc_err)?;
            Ok(json!({ "ok": true, "dropped": item_json(&item)? }))
        }
        "radar.pull" => {
            let item_id = get_string_param(params, "item_id")?;
            let plan_id = plan_refs::resolve_plan(store, params)?;
            svc.authorize_draft_structure_mutation(&plan_id, &authenticated_caller(p))
                .map_err(|message| JsonRpcError {
                    code: PLAN_ORCHESTRATOR_REQUIRED,
                    message,
                    data: None,
                })?;
            let section_ids = store
                .resolve_task_section_ids(&plan_id, &csv_string_field(p, "sections"))
                .map_err(plan_rpc_err)?;
            let item = store
                .pull_radar_item(
                    RadarPullRequest {
                        item_id,
                        plan_id: plan_id.clone(),
                        kind: optional_string(p, "type"),
                        section_ids,
                    },
                    now,
                )
                .map_err(radar_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            let task_id = item.pulled.as_ref().map(|pull| pull.task_id.clone());
            Ok(json!({
                "item": item_json(&item)?,
                "plan_id": plan_id,
                "task_id": task_id,
            }))
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
