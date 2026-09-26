use super::JsonRpcError;

pub(super) fn invalid_model_rpc_error(
    error: crate::service::spawn::InvalidModelError,
) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: error.to_string(),
        data: Some(error.rpc_data()),
    }
}
