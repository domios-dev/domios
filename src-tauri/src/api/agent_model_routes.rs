use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab, AppState};
use crate::service::agent_restart::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence, ModelSwitchOutcome,
};

#[derive(Debug, Deserialize)]
pub struct AgentModelBody {
    pub model: String,
}

pub async fn agent_model_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<AgentModelBody>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message.to_string()).into_response();
    }
    let tab_id = match resolve_tab(&state, &raw_tab_id) {
        Ok(tab_id) => tab_id,
        Err((status, message)) => {
            let error = ModelSwitchError::NotFound(message.to_string());
            return (status, Json(model_switch_error_body(&error))).into_response();
        }
    };
    let svc = state.svc.clone();
    let result =
        tokio::task::spawn_blocking(move || svc.agent_switch_model(&tab_id, &body.model)).await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let error =
                ModelSwitchError::NotSwitchable(format!("model switch worker failed: {error}"));
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(model_switch_error_body(&error)),
            )
                .into_response();
        }
    };
    match result {
        Ok(outcome) => (StatusCode::OK, Json(model_switch_success_body(&outcome))).into_response(),
        Err(error) => {
            let mut response = (
                model_switch_error_status(&error),
                Json(model_switch_error_body(&error)),
            )
                .into_response();
            if let ModelSwitchError::RateLimited { retry_after_secs } = error {
                if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                    response.headers_mut().insert("Retry-After", value);
                }
            }
            response
        }
    }
}

pub(crate) fn model_switch_success_body(outcome: &ModelSwitchOutcome) -> serde_json::Value {
    let mut body = serde_json::json!({
        "ok": true,
        "tab_id": outcome.restart.tab_id,
        "provider": outcome.restart.provider,
        "mechanism": outcome.mechanism,
        "previous_model": outcome.previous_model,
        "requested_model": outcome.requested_model,
        "launched_model": outcome.launched_model,
        "model_confirmation": match &outcome.confirmation {
            ModelSwitchConfirmation::Observed { .. } => "observed",
            ModelSwitchConfirmation::Pending => "pending",
        },
        "session_id": outcome.restart.session_id,
        "relaunch_evidence": {
            "session_id": outcome.restart.session_id,
            "launched_model": outcome.launched_model,
            "agent_ready": outcome.restart.agent_ready,
        },
        "restart": crate::api::agent_routes::agent_restart_response(&outcome.restart),
    });
    if let ModelSwitchConfirmation::Observed { model, evidence } = &outcome.confirmation {
        body["observed_model"] = model.clone().into();
        body["evidence"] = match evidence {
            ModelSwitchEvidence::SessionStartReport => "session_start_report",
            ModelSwitchEvidence::FirstPostRelaunchTurn => "first_post_relaunch_turn",
        }
        .into();
    }
    body
}

pub(crate) fn model_switch_error_status(error: &ModelSwitchError) -> StatusCode {
    match error {
        ModelSwitchError::NotFound(_) => StatusCode::NOT_FOUND,
        ModelSwitchError::InvalidModel(_) => StatusCode::BAD_REQUEST,
        ModelSwitchError::Unsupported { .. } | ModelSwitchError::NotSwitchable(_) => {
            StatusCode::CONFLICT
        }
        ModelSwitchError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        ModelSwitchError::RelaunchFailed {
            timed_out: true, ..
        } => StatusCode::GATEWAY_TIMEOUT,
        ModelSwitchError::RelaunchFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        ModelSwitchError::ModelMismatch { .. } => StatusCode::BAD_GATEWAY,
    }
}

pub(crate) fn model_switch_error_body(error: &ModelSwitchError) -> serde_json::Value {
    let reason = match error {
        ModelSwitchError::NotFound(_) => "not_found",
        ModelSwitchError::Unsupported { .. } => "model_switch_unsupported",
        ModelSwitchError::InvalidModel(_) => "invalid_model",
        ModelSwitchError::NotSwitchable(_) => "not_switchable",
        ModelSwitchError::RateLimited { .. } => "rate_limited",
        ModelSwitchError::RelaunchFailed { .. } => "relaunch_failed",
        ModelSwitchError::ModelMismatch { .. } => "model_mismatch",
    };
    let mut body = serde_json::json!({
        "ok": false,
        "error": error.to_string(),
        "reason": reason,
    });
    match error {
        ModelSwitchError::Unsupported { provider } => {
            body["provider"] = provider.clone().into();
        }
        ModelSwitchError::InvalidModel(invalid) => {
            body["model_error"] = invalid.rpc_data();
        }
        ModelSwitchError::RateLimited { retry_after_secs } => {
            body["retry_after_secs"] = (*retry_after_secs).into();
        }
        ModelSwitchError::RelaunchFailed { outcome, .. } => {
            body["restart"] = crate::api::agent_routes::agent_restart_response(outcome);
        }
        ModelSwitchError::ModelMismatch {
            outcome,
            launched_model,
            observed_model,
        } => {
            body["launched_model"] = launched_model.clone().into();
            body["observed_model"] = observed_model.clone().into();
            body["restart"] = crate::api::agent_routes::agent_restart_response(outcome);
        }
        ModelSwitchError::NotFound(_) | ModelSwitchError::NotSwitchable(_) => {}
    }
    body
}

#[cfg(test)]
mod tests;
