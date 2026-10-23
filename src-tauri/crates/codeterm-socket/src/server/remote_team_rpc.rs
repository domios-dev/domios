use super::{get_string_param, opt_string_param, JsonRpcError};
use codeterm_service_core::service::{AuthenticatedPlanCaller, ServiceState};
use serde_json::Value;

pub(super) fn caller(params: &Option<Value>) -> AuthenticatedPlanCaller {
    AuthenticatedPlanCaller {
        tab_id: opt_string_param(params, "caller_tab_id"),
        session_id: opt_string_param(params, "session_id"),
        binding_nonce: opt_string_param(params, "session_binding_nonce"),
    }
}

pub(super) fn error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message,
        data: None,
    }
}

pub(super) fn reparent(params: &Option<Value>, svc: &ServiceState) -> Result<Value, JsonRpcError> {
    let worker = get_string_param(params, "tab_id")?;
    let destination = get_string_param(params, "to")?;
    let peer = opt_string_param(params, "peer");
    codeterm_service_core::async_runtime::block_on(svc.reparent_worker(
        caller(params),
        worker,
        destination,
        peer,
    ))
    .map_err(error)
}
