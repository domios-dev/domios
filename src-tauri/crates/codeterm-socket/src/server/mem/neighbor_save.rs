use super::*;

pub(super) fn save(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let caller = params.as_ref().and_then(|params| {
        Some(codeterm_service_core::service::tab_credentials::TabCaller {
            tab_id: params["caller_tab_id"].as_str()?.into(),
            binding_nonce: params["session_binding_nonce"].as_str()?.into(),
        })
    });
    let body = parse_params(params)?;
    codeterm_service_core::async_runtime::block_on(svc.save_neighbor_memory(
        caller, codeterm_service_core::mem::store::current_author(), body,
    )).map_err(|message| JsonRpcError {
        code: -32003,
        data: Some(serde_json::json!({"reason":message.split(':').next().unwrap_or("neighbor_memory_write_not_allowed")})),
        message,
    })
}
