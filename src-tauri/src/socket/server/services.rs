//! `services.*` JSON-RPC handlers.

use serde_json::Value;
use tauri::AppHandle;

use crate::service::ServiceState;
use crate::services::activity::{ActivitySource, ActivityWriter};
use crate::services::store::{ServiceCreate, ServiceKind, ServiceUpdate};

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "services.list" => {
            let kind = params
                .as_ref()
                .and_then(|p| p.get("kind"))
                .and_then(|v| v.as_str())
                .and_then(parse_kind);
            let enabled = params
                .as_ref()
                .and_then(|p| p.get("enabled"))
                .and_then(|v| v.as_bool());
            let services = svc
                .services_store
                .list(kind, enabled)
                .map_err(internal_err)?;
            serde_json::to_value(&services).map_err(|e| internal_err(e.to_string()))
        }

        "services.create" => {
            let p = params
                .clone()
                .ok_or_else(|| invalid_params("params required"))?;
            let input: ServiceCreate = serde_json::from_value(p)
                .map_err(|e| invalid_params(format!("Invalid params: {}", e)))?;
            if input.kind != ServiceKind::Daemon {
                return Err(invalid_params(
                    "Only daemon-kind services can be created via this method; use cron.create for cron jobs",
                ));
            }
            let id = svc.services_store.create(input).map_err(internal_err)?;
            let token = svc.tokens.generate(&id).map_err(internal_err)?;
            let service = svc
                .services_store
                .get(&id)
                .map_err(internal_err)?
                .ok_or_else(|| internal_err("service vanished after create"))?;
            Ok(serde_json::json!({ "service": service, "token": token }))
        }

        "services.get" => {
            let id = get_string_param(params, "id")?;
            let service = svc.services_store.get(&id).map_err(internal_err)?;
            serde_json::to_value(&service).map_err(|e| internal_err(e.to_string()))
        }

        "services.update" => {
            let id = get_string_param(params, "id")?;
            let p = params
                .clone()
                .ok_or_else(|| invalid_params("params required"))?;
            let patch: ServiceUpdate = serde_json::from_value(p)
                .map_err(|e| invalid_params(format!("Invalid params: {}", e)))?;
            svc.services_store
                .update(&id, patch)
                .map_err(internal_err)?;
            let service = svc
                .services_store
                .get(&id)
                .map_err(internal_err)?
                .ok_or_else(|| internal_err(format!("service {} not found after update", id)))?;
            serde_json::to_value(&service).map_err(|e| internal_err(e.to_string()))
        }

        "services.delete" => {
            let id = get_string_param(params, "id")?;
            svc.services_store.delete(&id).map_err(internal_err)?;
            let _ = svc.tokens.delete(&id);
            Ok(serde_json::json!({ "ok": true }))
        }

        "services.regenerate_token" => {
            let id = get_string_param(params, "id")?;
            let token = svc.tokens.generate(&id).map_err(internal_err)?;
            let _ = svc.services_store.update(
                &id,
                ServiceUpdate { push_enabled: Some(true), ..Default::default() },
            );
            Ok(serde_json::json!({ "token": token, "push_enabled": true }))
        }

        "activity.list" => {
            let source = params
                .as_ref()
                .and_then(|p| p.get("source"))
                .and_then(|v| v.as_str())
                .and_then(parse_activity_source);
            let since = params
                .as_ref()
                .and_then(|p| p.get("since"))
                .and_then(|v| v.as_i64());
            let limit = params
                .as_ref()
                .and_then(|p| p.get("limit"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let writer = ActivityWriter::new(svc.services_store.conn());
            let events = writer.list(source, since, limit).map_err(internal_err)?;
            serde_json::to_value(&events).map_err(|e| internal_err(e.to_string()))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}

fn parse_kind(s: &str) -> Option<ServiceKind> {
    match s {
        "cron" => Some(ServiceKind::Cron),
        "daemon" => Some(ServiceKind::Daemon),
        _ => None,
    }
}

fn parse_activity_source(s: &str) -> Option<ActivitySource> {
    match s {
        "push" => Some(ActivitySource::Push),
        "health" => Some(ActivitySource::Health),
        "peer" => Some(ActivitySource::Peer),
        "cron" => Some(ActivitySource::Cron),
        _ => None,
    }
}

fn internal_err<E: ToString>(e: E) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
    }
}

fn invalid_params<S: Into<String>>(msg: S) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: msg.into(),
    }
}
