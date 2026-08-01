//! `plan.handoff.*` JSON-RPC handlers.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::handoff::{HandoffCreateRequest, HandoffReseedRequest};
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::JsonRpcError;

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

fn provider_command_for_reseed(
    svc: &ServiceState,
    tab_id: &str,
    mode: &str,
) -> Result<String, JsonRpcError> {
    let provider_id = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .and_then(|tab| tab.agent_type.clone())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: format!("tab {tab_id} has no agent provider"),
                data: None,
            })?
    };
    svc.agent_registry
        .with_provider(&provider_id, |provider| {
            if mode == "compact" {
                provider.compact_command()
            } else {
                provider.fresh_context_command()
            }
            .map(str::to_string)
        })
        .flatten()
        .ok_or_else(|| JsonRpcError {
            code: -32603,
            message: format!("provider {provider_id} does not support handoff mode {mode}"),
            data: None,
        })
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
        "plan.handoff.create" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request: HandoffCreateRequest =
                serde_json::from_value(body.clone()).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid handoff create params: {e}"),
                    data: None,
                })?;
            let result = store
                .create_handoff(&id, request, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.handoff.list" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let result = store.list_handoffs(&id).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.handoff.show" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let selector = p.and_then(|x| x.get("selector")).and_then(|v| v.as_str());
            let result = store.show_handoff(&id, selector).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.handoff.reseed" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let body = p.ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request: HandoffReseedRequest =
                serde_json::from_value(body.clone()).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid handoff reseed params: {e}"),
                    data: None,
                })?;
            let command = provider_command_for_reseed(svc, &request.tab_id, &request.mode)?;
            let detail = store
                .show_handoff(&id, request.selector.as_deref())
                .map_err(plan_rpc_err)?;
            let seed = detail
                .content
                .get("resume_prompt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let seed = store.prepend_heartbeat_for_plan(&id, &seed);
            let result = store
                .prepare_handoff_reseed(&id, &request, &command, &seed, now)
                .map_err(plan_rpc_err)?;
            if !request.dry_run {
                for send in result.sends.iter().filter(|send| {
                    (result.mode == "new" && send.order == 1) || result.mode == "compact"
                }) {
                    if send.enter {
                        svc.send_text_detached(&result.tab_id, &send.text);
                    } else {
                        svc.insert_text_detached(&result.tab_id, &send.text);
                    }
                }
                emit_plan_changed(app, &id);
            }
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
