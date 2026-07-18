use crate::api::mem_routes::search::{execute_search, SearchParams};
use crate::service::ServiceState;

use super::JsonRpcError;

pub(super) fn dispatch(
    method: &str,
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    match method {
        "mem.stats" => svc
            .mem
            .stats()
            .and_then(|stats| serde_json::to_value(stats).map_err(|error| error.to_string()))
            .map_err(internal_error),
        "mem.search" => {
            let request = serde_json::from_value::<SearchParams>(params.unwrap_or_default())
                .map_err(|error| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid mem.search parameters: {error}"),
                    data: None,
                })?;
            execute_search(svc, request).map_err(internal_error)
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn internal_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message,
        data: None,
    }
}
