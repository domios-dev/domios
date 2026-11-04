use super::JsonRpcError;

pub(super) fn invalid_model_rpc_error(
    error: codeterm_service_core::service::spawn::InvalidModelError,
) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: error.to_string(),
        data: Some(error.rpc_data()),
    }
}

pub(super) fn launch_profile_rpc_error(
    error: codeterm_service_core::service::spawn_params::LaunchProfileRefusal,
) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: error.message.clone(),
        data: Some(error.data()),
    }
}

pub(super) fn spawn_target_rpc_error(
    error: codeterm_service_core::service::spawn_params::SpawnTargetRefusal,
) -> JsonRpcError {
    let data = error.data();
    JsonRpcError {
        code: if data.is_some() { -32602 } else { -32603 },
        message: error.to_string(),
        data,
    }
}

pub(super) fn spawn_rpc_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message,
        data: None,
    }
}

#[cfg(test)]
mod tests;
