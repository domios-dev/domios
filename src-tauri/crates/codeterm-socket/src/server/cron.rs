//! `cron.*` JSON-RPC handlers.

use codeterm_service_core::app_runtime::AppHandle;
use serde_json::Value;
use tauri::Manager;

use codeterm_service_core::service::ServiceState;

use super::{get_string_param, JsonRpcError, PostResponseAction};

pub(super) fn dispatch_complete(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<(Value, Option<PostResponseAction>), JsonRpcError> {
    let run_id = get_string_param(params, "run_id")?;
    let summary = params
        .as_ref()
        .and_then(|p| p.get("summary"))
        .and_then(|v| v.as_str());
    let caller_tab_id = params
        .as_ref()
        .and_then(|p| p.get("caller_tab_id"))
        .and_then(|v| v.as_str());
    let binding_nonce = params
        .as_ref()
        .and_then(|p| p.get("session_binding_nonce"))
        .and_then(|v| v.as_str());
    let human_confirmed = params
        .as_ref()
        .and_then(|p| p.get("human_confirmed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cron_state = app
        .try_state::<codeterm_service_core::CronState>()
        .ok_or_else(|| JsonRpcError {
            code: -32603,
            message: "Cron not initialized".into(),
            data: None,
        })?;
    let cron_wake = app
        .try_state::<codeterm_service_core::CronWake>()
        .ok_or_else(|| JsonRpcError {
            code: -32603,
            message: "Cron not initialized".into(),
            data: None,
        })?;
    let completion = svc
        .cron_complete(
            &cron_state,
            &cron_wake,
            &run_id,
            summary,
            caller_tab_id,
            binding_nonce,
            human_confirmed,
        )
        .map_err(|e| JsonRpcError {
            code: -32603,
            message: e,
            data: None,
        })?;
    let after_response = completion.map(|completion| {
        let svc = svc.clone();
        Box::new(move || {
            let _ = svc.close_tab_with_reason(
                &completion.tab_id,
                codeterm_service_core::multiplexer::types::TabCloseReason::AgentRunEnded,
            );
            let svc = svc.clone();
            std::thread::spawn(move || {
                svc.cron_settle_run_end(
                    &completion.cron,
                    Some(&completion.wake),
                    codeterm_service_core::service::cron::CronRunEnd {
                        job_id: &completion.job_id,
                        run_id: &completion.run_id,
                        status: codeterm_service_core::cron::store::CronRunStatus::Success,
                        summary: &completion.summary,
                    },
                );
                codeterm_service_core::events::emit_everywhere(
                    &svc.app_handle,
                    "cron-run-finished",
                    (),
                );
            });
        }) as PostResponseAction
    });
    Ok((serde_json::json!({"ok": true}), after_response))
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "cron.list" => {
            let ws_id = params
                .as_ref()
                .and_then(|p| p.get("workspace_id"))
                .and_then(|v| v.as_str());
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let jobs = svc
                .cron_list(&cron_state, ws_id)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            serde_json::to_value(&jobs).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.get" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let job = svc
                .cron_get(&cron_state, &id)
                .map_err(|message| JsonRpcError {
                    code: if message.starts_with("cron storage unavailable:") {
                        -32603
                    } else {
                        -32602
                    },
                    message,
                    data: None,
                })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.create" => {
            let p = params.clone().ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "params required".into(),
                data: None,
            })?;
            let create_params: codeterm_service_core::cron::store::CronCreateParams =
                serde_json::from_value(p).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", e),
                    data: None,
                })?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cron_wake = app
                .try_state::<codeterm_service_core::CronWake>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let job = svc
                .cron_create(&cron_state, &cron_wake, create_params)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.update" => {
            let id = get_string_param(params, "id")?;
            let p = params.clone().ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "params required".into(),
                data: None,
            })?;
            let patch: codeterm_service_core::cron::store::CronUpdateParams =
                serde_json::from_value(p).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", e),
                    data: None,
                })?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cron_wake = app
                .try_state::<codeterm_service_core::CronWake>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let job = svc
                .cron_update(&cron_state, &cron_wake, &id, patch)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.delete" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cron_wake = app
                .try_state::<codeterm_service_core::CronWake>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            svc.cron_delete(&cron_state, &cron_wake, &id)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            Ok(serde_json::json!({"ok": true}))
        }

        "cron.enable" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cron_wake = app
                .try_state::<codeterm_service_core::CronWake>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let job = svc
                .cron_enable(&cron_state, &cron_wake, &id, true)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.disable" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cron_wake = app
                .try_state::<codeterm_service_core::CronWake>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let job = svc
                .cron_enable(&cron_state, &cron_wake, &id, false)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.trigger" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let run = svc
                .cron_trigger(&cron_state, &id)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            serde_json::to_value(&run).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.cancel" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let cancelled =
                svc.cron_cancel_running(&cron_state, &id)
                    .map_err(|e| JsonRpcError {
                        code: -32603,
                        message: e,
                        data: None,
                    })?;
            Ok(serde_json::json!({"cancelled": cancelled}))
        }

        "cron.history" => {
            let id = get_string_param(params, "id")?;
            let cron_state = app
                .try_state::<codeterm_service_core::CronState>()
                .ok_or_else(|| JsonRpcError {
                    code: -32603,
                    message: "Cron not initialized".into(),
                    data: None,
                })?;
            let history = svc
                .cron_history(&cron_state, &id)
                .map_err(|message| JsonRpcError {
                    code: if message.starts_with("cron storage unavailable:") {
                        -32603
                    } else {
                        -32602
                    },
                    message,
                    data: Some(serde_json::json!({ "code": "not_found" })),
                })?;
            serde_json::to_value(&history).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        "cron.complete" => {
            let (result, after_response) = dispatch_complete(params, app, svc)?;
            if let Some(after_response) = after_response {
                after_response();
            }
            Ok(result)
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
