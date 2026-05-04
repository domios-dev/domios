//! `cron.*` JSON-RPC handlers.

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "cron.list" => {
            let ws_id = params.as_ref().and_then(|p| p.get("workspace_id")).and_then(|v| v.as_str());
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let jobs = svc.cron_list(&cron_state, ws_id);
            serde_json::to_value(&jobs).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.get" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_get(&cron_state, &id).map_err(|e| JsonRpcError { code: -32602, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.create" => {
            let p = params.clone().ok_or_else(|| JsonRpcError { code: -32602, message: "params required".into() })?;
            let create_params: crate::cron::store::CronCreateParams = serde_json::from_value(p)
                .map_err(|e| JsonRpcError { code: -32602, message: format!("Invalid params: {}", e) })?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_create(&cron_state, &cron_wake, create_params)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.update" => {
            let id = get_string_param(params, "id")?;
            let p = params.clone().ok_or_else(|| JsonRpcError { code: -32602, message: "params required".into() })?;
            let patch: crate::cron::store::CronUpdateParams = serde_json::from_value(p)
                .map_err(|e| JsonRpcError { code: -32602, message: format!("Invalid params: {}", e) })?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_update(&cron_state, &cron_wake, &id, patch)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.delete" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            svc.cron_delete(&cron_state, &cron_wake, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"ok": true}))
        }

        "cron.enable" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_enable(&cron_state, &cron_wake, &id, true)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.disable" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_enable(&cron_state, &cron_wake, &id, false)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.trigger" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let run = svc.cron_trigger(&cron_state, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&run).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.cancel" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cancelled = svc.cron_cancel_running(&cron_state, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"cancelled": cancelled}))
        }

        "cron.history" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let history = svc.cron_history(&cron_state, &id);
            serde_json::to_value(&history).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.complete" => {
            let run_id = get_string_param(params, "run_id")?;
            let summary = params.as_ref().and_then(|p| p.get("summary")).and_then(|v| v.as_str());
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            svc.cron_complete(&cron_state, &cron_wake, &run_id, summary)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
