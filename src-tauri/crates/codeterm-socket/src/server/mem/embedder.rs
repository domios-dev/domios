use codeterm_service_core::mem_ops::embedder::{self, UseParams};
use codeterm_service_core::service::ServiceState;

use super::{internal_error, parse_params, JsonRpcError};

/// Gate refusals are answers, not transport failures: they carry the hints
/// the caller prints.
pub(super) fn dispatch(
    method: &str,
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let value = match method {
        "list" => serde_json::to_value(embedder::list(svc)),
        "status" => serde_json::to_value(svc.mem.embedder_status()),
        "preview" => {
            let params: UseParams = parse_params(params)?;
            serde_json::to_value(embedder::preview(svc, &params).map_err(refused)?)
        }
        "use" => {
            let params: UseParams = parse_params(params)?;
            serde_json::to_value(embedder::use_provider(svc, &params).map_err(refused)?)
        }
        _ => {
            return Err(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            })
        }
    };
    value.map_err(|error| internal_error(error.to_string()))
}

fn refused(refusal: embedder::Refusal) -> JsonRpcError {
    JsonRpcError {
        code: -32009,
        message: refusal.message.clone(),
        data: Some(serde_json::json!({ "hints": refusal.hints })),
    }
}
