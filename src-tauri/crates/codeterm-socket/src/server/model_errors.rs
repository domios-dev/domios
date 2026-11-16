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

/// A Team manager admission refusal is the caller's request, not a daemon fault.
const MANAGER_ADMISSION_REFUSALS: [&str; 3] = [
    codeterm_types::team::staffing::TEAM_ROLE_ALREADY_LIVE,
    codeterm_types::team::staffing::TEAM_ROLE_REQUIRES_ORCHESTRATOR,
    codeterm_types::team::staffing::TEAM_ROLE_NOT_TASK_BOUND,
];

pub(super) fn spawn_rpc_error(message: String) -> JsonRpcError {
    let refusal = message.split_once(':').and_then(|(code, _)| {
        MANAGER_ADMISSION_REFUSALS
            .into_iter()
            .find(|known| *known == code)
    });
    match refusal {
        Some(code) => JsonRpcError {
            code: -32602,
            message,
            data: Some(serde_json::json!({ "code": code })),
        },
        None => JsonRpcError {
            code: -32603,
            message,
            data: None,
        },
    }
}

#[cfg(test)]
mod tests;
