use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::service::agent_restart::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence, ModelSwitchOutcome,
};

#[derive(Debug, Deserialize)]
pub struct AgentModelBody {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub cancel: bool,
    pub reasoning_effort: Option<String>,
    pub runtime_mode: Option<codeterm_service_core::service::agent_runtime::model::RuntimeMode>,
    #[serde(default)]
    pub restart_resume: bool,
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
    if body.cancel {
        svc.cancel_staged_model_switch(&tab_id);
        return Json(serde_json::json!({"ok": true, "cancelled": true})).into_response();
    }
    let result = tokio::task::spawn_blocking(move || {
        svc.agent_switch_model_with_restart_fallback(
            &tab_id,
            &body.model,
            body.reasoning_effort.as_deref(),
            body.runtime_mode,
            codeterm_service_core::service::agent_runtime::model::ModelSwitchOptions {
                force: body.force,
                restart_resume: body.restart_resume,
            },
        )
    })
    .await;
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
        "tab_id": outcome.tab_id,
        "provider": outcome.provider,
        "mechanism": outcome.mechanism,
        "mode_used": outcome.mechanism,
        "fallback_reason": outcome.fallback_reason,
        "previous_model": outcome.previous_model,
        "requested_model": outcome.requested_model,
        "launched_model": outcome.launched_model,
        "model_confirmation": match &outcome.confirmation {
            ModelSwitchConfirmation::Observed { .. } => "observed",
            ModelSwitchConfirmation::Pending | ModelSwitchConfirmation::AfterTurn => "pending",
        },
        "session_id": outcome.session_id,
        "staged": matches!(outcome.confirmation, ModelSwitchConfirmation::AfterTurn),
    });
    if let Some(detail) = &outcome.confirmation_detail {
        body["confirmation_detail"] = detail.clone().into();
    }
    if let Some(restart) = &outcome.restart {
        body["relaunch_evidence"] = serde_json::json!({
            "session_id": restart.session_id,
            "launched_model": outcome.launched_model,
            "agent_ready": restart.agent_ready,
        });
        body["restart"] = crate::agent_routes::agent_restart_response(restart);
    }
    if let ModelSwitchConfirmation::Observed { model, evidence } = &outcome.confirmation {
        body["observed_model"] = model.clone().into();
        body["evidence"] = match evidence {
            ModelSwitchEvidence::LiveSessionRecord => "live_session_record",
            ModelSwitchEvidence::SessionStartReport => "session_start_report",
            ModelSwitchEvidence::FirstPostRelaunchTurn => "first_post_relaunch_turn",
        }
        .into();
    }
    body
}

pub(crate) fn model_switch_error_status(error: &ModelSwitchError) -> StatusCode {
    match error {
        ModelSwitchError::BackgroundWork { .. } | ModelSwitchError::RuntimeControlRefused(_) => {
            StatusCode::CONFLICT
        }
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
        ModelSwitchError::LiveSwitchFailed(_)
        | ModelSwitchError::LiveCommandFailed(_)
        | ModelSwitchError::InputNotReady(_) => StatusCode::BAD_GATEWAY,
    }
}

pub(crate) fn model_switch_error_body(error: &ModelSwitchError) -> serde_json::Value {
    let reason = match error {
        ModelSwitchError::BackgroundWork { .. } => "background_work_running",
        ModelSwitchError::NotFound(_) => "not_found",
        ModelSwitchError::Unsupported { .. } => "model_switch_unsupported",
        ModelSwitchError::InvalidModel(_) => "invalid_model",
        ModelSwitchError::NotSwitchable(_) => "not_switchable",
        ModelSwitchError::RateLimited { .. } => "rate_limited",
        ModelSwitchError::RelaunchFailed { .. } => "relaunch_failed",
        ModelSwitchError::ModelMismatch { .. } => "model_mismatch",
        ModelSwitchError::LiveSwitchFailed(_) => "live_switch_failed",
        ModelSwitchError::LiveCommandFailed(_) => "live_command_failed",
        ModelSwitchError::InputNotReady(_) => "input_not_ready",
        ModelSwitchError::RuntimeControlRefused(_) => "runtime_control_refused",
    };
    let mut body = serde_json::json!({
        "ok": false,
        "error": error.to_string(),
        "reason": reason,
    });
    if matches!(
        error,
        ModelSwitchError::LiveSwitchFailed(_) | ModelSwitchError::RuntimeControlRefused(_)
    ) {
        body["mode_used"] = "live".into();
    }
    if matches!(
        error,
        ModelSwitchError::LiveCommandFailed(_) | ModelSwitchError::InputNotReady(_)
    ) {
        body["mode_used"] = "live_command".into();
        body["restart_fallback_available"] = true.into();
    }
    match error {
        ModelSwitchError::BackgroundWork { items } => {
            body["running_items"] = serde_json::json!(items);
        }
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
            body["restart"] = crate::agent_routes::agent_restart_response(outcome);
        }
        ModelSwitchError::ModelMismatch {
            outcome,
            launched_model,
            observed_model,
        } => {
            body["launched_model"] = launched_model.clone().into();
            body["observed_model"] = observed_model.clone().into();
            body["restart"] = crate::agent_routes::agent_restart_response(outcome);
        }
        ModelSwitchError::NotFound(_)
        | ModelSwitchError::NotSwitchable(_)
        | ModelSwitchError::LiveSwitchFailed(_)
        | ModelSwitchError::LiveCommandFailed(_)
        | ModelSwitchError::InputNotReady(_)
        | ModelSwitchError::RuntimeControlRefused(_) => {}
    }
    body
}

#[cfg(test)]
mod tests;
