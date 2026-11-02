use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::service::agent_restart::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence, ModelSwitchOutcome,
};

pub async fn agent_service_tiers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message.to_string()).into_response();
    }
    let tab_id = match crate::routes::resolve_tab_async(&state, &raw_tab_id).await {
        Ok(tab_id) => tab_id,
        Err((status, message)) => {
            return (
                status,
                Json(serde_json::json!({"ok":false,"error":message,"reason":"not_found"})),
            )
                .into_response();
        }
    };
    let mux = state.svc.mux.read();
    let Some(tab) = mux.get_tab(&tab_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"ok":false,"error":"Tab not found","reason":"not_found"})),
        )
            .into_response();
    };
    let Some(config) = tab.agent_runtime_config.as_ref() else {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"ok":false,"error":"Tab has no agent runtime settings","reason":"no_runtime"}))).into_response();
    };
    match service_tier_catalogue(
        config,
        tab.agent_session_model.as_deref(),
        tab.agent_session_id.as_deref(),
        tab.session_binding_nonce.as_deref(),
    ) {
        Ok(catalogue) => {
            let mut body = catalogue;
            body["tab_id"] = tab_id.into();
            Json(body).into_response()
        }
        Err((status, error)) => (status, Json(error)).into_response(),
    }
}

fn service_tier_catalogue(
    config: &Value,
    session_model: Option<&str>,
    session_id: Option<&str>,
    binding_nonce: Option<&str>,
) -> Result<Value, (StatusCode, Value)> {
    let binding = config
        .get("serviceTierAvailabilityBinding")
        .unwrap_or(&Value::Null);
    let binding_matches = binding.get("sessionId").and_then(Value::as_str) == session_id
        && binding
            .get("bindingNonce")
            .and_then(Value::as_str)
            .is_some_and(|nonce| !nonce.is_empty() && Some(nonce) == binding_nonce);
    if !binding_matches {
        return Err((
            StatusCode::CONFLICT,
            serde_json::json!({"ok":false,"error":"No service tier catalogue is bound to this runtime session","reason":"no_tiers"}),
        ));
    }
    let model = config
        .get("model")
        .and_then(Value::as_str)
        .or(session_model)
        .unwrap_or_default();
    let tiers = config
        .get("serviceTierAvailability")
        .and_then(|value| value.get(model))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|tier| tier.get("unavailableReason").is_none())
        .collect::<Vec<_>>();
    if tiers.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            serde_json::json!({"ok":false,"error":"This runtime model advertises no available service tiers","reason":"no_tiers"}),
        ));
    }
    let confirmed = config
        .get("confirmedServiceTier")
        .filter(|value| {
            value.get("sessionId").and_then(Value::as_str) == session_id
                && value.get("bindingNonce").and_then(Value::as_str) == binding_nonce
        })
        .and_then(|value| value.get("selection"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(
        serde_json::json!({"ok":true,"model":model,"supports_service_tiers":true,"tiers":tiers,
        "requested":config.get("serviceTier").cloned().unwrap_or(Value::Null),"confirmed":confirmed}),
    )
}

#[cfg(test)]
mod control_error_tests {
    use super::*;

    #[test]
    fn unsupported_runtime_controls_are_bad_requests_with_supported_values() {
        for (error, reason, supported) in [
            (
                ModelSwitchError::UnsupportedReasoningEffort {
                    model: "gpt-6-luna".into(),
                    requested: "qa-impossible".into(),
                    supported: vec!["low".into(), "medium".into()],
                },
                "unsupported_reasoning_effort",
                vec!["low", "medium"],
            ),
            (
                ModelSwitchError::UnsupportedRuntimeMode {
                    requested: "qa-impossible".into(),
                    supported: vec!["auto".into(), "full-access".into()],
                },
                "unsupported_runtime_mode",
                vec!["auto", "full-access"],
            ),
        ] {
            let body = model_switch_error_body(&error);
            assert_eq!(model_switch_error_status(&error), StatusCode::BAD_REQUEST);
            assert_eq!(body["reason"], reason);
            assert_eq!(body["supported"], serde_json::json!(supported));
            assert_eq!(body["ok"], false);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentModelBody {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub cancel: bool,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<codeterm_plugin::agent_runtime::RuntimeServiceTierSelection>,
    pub runtime_mode: Option<String>,
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
    let tab_id = match crate::routes::resolve_tab_async(&state, &raw_tab_id).await {
        Ok(tab_id) => tab_id,
        Err((status, message)) => {
            let error = ModelSwitchError::NotFound(message.to_string());
            return (status, Json(model_switch_error_body(&error))).into_response();
        }
    };
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        if body.cancel {
            svc.cancel_staged_model_switch(&tab_id);
            return None;
        }
        Some(svc.agent_switch_model_with_restart_fallback(
            &tab_id,
            &body.model,
            body.reasoning_effort.as_deref(),
            body.runtime_mode.as_deref(),
            codeterm_service_core::service::agent_runtime::model::ModelSwitchOptions {
                service_tier: body.service_tier,
                force: body.force,
                restart_resume: body.restart_resume,
            },
        ))
    })
    .await;
    let result = match result {
        Ok(Some(result)) => result,
        Ok(None) => {
            return Json(serde_json::json!({"ok": true, "cancelled": true})).into_response();
        }
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
        ModelSwitchError::UnsupportedReasoningEffort { .. }
        | ModelSwitchError::UnsupportedRuntimeMode { .. } => StatusCode::BAD_REQUEST,
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
        | ModelSwitchError::InputNotReady(_)
        | ModelSwitchError::DuringTurnIncident { .. } => StatusCode::BAD_GATEWAY,
    }
}

pub(crate) fn model_switch_error_body(error: &ModelSwitchError) -> serde_json::Value {
    let reason = match error {
        ModelSwitchError::BackgroundWork { .. } => "background_work_running",
        ModelSwitchError::NotFound(_) => "not_found",
        ModelSwitchError::Unsupported { .. } => "model_switch_unsupported",
        ModelSwitchError::InvalidModel(_) => "invalid_model",
        ModelSwitchError::UnsupportedReasoningEffort { .. } => "unsupported_reasoning_effort",
        ModelSwitchError::UnsupportedRuntimeMode { .. } => "unsupported_runtime_mode",
        ModelSwitchError::NotSwitchable(_) => "not_switchable",
        ModelSwitchError::RateLimited { .. } => "rate_limited",
        ModelSwitchError::RelaunchFailed { .. } => "relaunch_failed",
        ModelSwitchError::ModelMismatch { .. } => "model_mismatch",
        ModelSwitchError::LiveSwitchFailed(_) => "live_switch_failed",
        ModelSwitchError::LiveCommandFailed(_) => "live_command_failed",
        ModelSwitchError::DuringTurnIncident { .. } => "during_turn_switch_incident",
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
        ModelSwitchError::DuringTurnIncident { stage, screen, .. } => {
            body["mode_used"] = "live_command".into();
            body["stage"] = stage.clone().into();
            body["screen"] = screen.clone().into();
            body["restart_fallback_available"] = false.into();
        }
        ModelSwitchError::BackgroundWork { items } => {
            body["running_items"] = serde_json::json!(items);
        }
        ModelSwitchError::Unsupported { provider } => {
            body["provider"] = provider.clone().into();
        }
        ModelSwitchError::InvalidModel(invalid) => {
            body["model_error"] = invalid.rpc_data();
        }
        ModelSwitchError::UnsupportedReasoningEffort {
            model,
            requested,
            supported,
        } => {
            body["model"] = model.clone().into();
            body["requested"] = requested.clone().into();
            body["supported"] = serde_json::json!(supported);
        }
        ModelSwitchError::UnsupportedRuntimeMode {
            requested,
            supported,
        } => {
            body["requested"] = requested.clone().into();
            body["supported"] = serde_json::json!(supported);
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

#[cfg(test)]
mod service_tier_tests {
    use super::*;

    #[test]
    fn exact_bound_catalogue_includes_available_tiers_and_both_selections() {
        let config = serde_json::json!({
            "model":"model-a",
            "serviceTier":{"id":"fast"},
            "serviceTierAvailability":{"model-a":[{"id":"fast"},{"id":"ultra","unavailableReason":"plan"}]},
            "serviceTierAvailabilityBinding":{"sessionId":"session-a","bindingNonce":"nonce-a"},
            "confirmedServiceTier":{"selection":{"id":"fast"},"sessionId":"session-a","bindingNonce":"nonce-a"}
        });
        let result =
            service_tier_catalogue(&config, None, Some("session-a"), Some("nonce-a")).unwrap();
        assert_eq!(result["tiers"], serde_json::json!([{"id":"fast"}]));
        assert_eq!(result["requested"]["id"], "fast");
        assert_eq!(result["confirmed"]["id"], "fast");
    }

    #[test]
    fn default_model_uses_bound_session_model() {
        let config = serde_json::json!({
            "model":null,
            "serviceTierAvailability":{"session-model":[{"id":"fast"}]},
            "serviceTierAvailabilityBinding":{"sessionId":"session-a","bindingNonce":"nonce-a"}
        });
        let result = service_tier_catalogue(
            &config,
            Some("session-model"),
            Some("session-a"),
            Some("nonce-a"),
        )
        .unwrap();
        assert_eq!(result["model"], "session-model");
        assert_eq!(result["tiers"], serde_json::json!([{"id":"fast"}]));
        assert!(service_tier_catalogue(
            &config,
            Some("session-model"),
            Some("foreign-session"),
            Some("nonce-a")
        )
        .is_err());
        assert!(service_tier_catalogue(
            &config,
            Some("session-model"),
            Some("session-a"),
            Some("foreign-nonce")
        )
        .is_err());
    }

    #[test]
    fn stale_binding_and_empty_availability_are_typed_no_tiers_refusals() {
        let config = serde_json::json!({"model":"model-a","serviceTierAvailabilityBinding":{"sessionId":"session-a","bindingNonce":"nonce-a"},"serviceTierAvailability":{"model-a":[]}});
        assert_eq!(
            service_tier_catalogue(&config, None, Some("session-b"), Some("nonce-a"))
                .unwrap_err()
                .1["reason"],
            "no_tiers"
        );
        assert_eq!(
            service_tier_catalogue(&config, None, Some("session-a"), Some("nonce-a"))
                .unwrap_err()
                .1["reason"],
            "no_tiers"
        );
    }
}
