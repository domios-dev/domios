use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::api_support::OptionalJson;

#[derive(Deserialize)]
pub(crate) struct AgentRestartBody {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_grace_secs")]
    grace_secs: u64,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    handoff_history: bool,
    #[serde(default)]
    fresh: bool,
}

fn default_grace_secs() -> u64 {
    codeterm_service_core::service::agent_restart::DEFAULT_GRACE_SECS
}

pub(crate) fn agent_restart_response(
    outcome: &codeterm_service_core::service::agent_restart::RestartOutcome,
) -> serde_json::Value {
    serde_json::json!({
        "restart_accepted": outcome.restart_accepted,
        "old_pty_torn_down": outcome.old_pty_torn_down,
        "agent_ready": outcome.agent_ready,
        "recovered_in_place": outcome.recovered_in_place,
        "old_pty_id": outcome.old_pty_id,
        "new_pty_id": outcome.new_pty_id,
        "tab_id": outcome.tab_id,
        "provider": outcome.provider,
        "session_id": outcome.session_id,
        "session_was_stale": outcome.session_was_stale,
        "session_action": outcome.session_action(),
        "full_access": outcome.auto_approve,
        "force": outcome.force,
    })
}

pub(crate) fn agent_restart_failure_response(
    outcome: &codeterm_service_core::service::agent_restart::RestartOutcome,
    reason: &str,
) -> serde_json::Value {
    let mut response = agent_restart_response(outcome);
    response["error"] = serde_json::Value::String(reason.to_string());
    response
}

pub(crate) async fn agent_restart_handler(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    OptionalJson(body): OptionalJson<AgentRestartBody>,
) -> axum::response::Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message.to_string()).into_response();
    }
    let tab_id = match resolve_tab(&state, &raw_tab_id) {
        Ok(tab_id) => tab_id,
        Err((status, message)) => return (status, message.to_string()).into_response(),
    };
    let body = body.unwrap_or(AgentRestartBody {
        force: false,
        grace_secs: default_grace_secs(),
        provider: None,
        handoff_history: false,
        fresh: false,
    });
    let svc = state.svc.clone();
    let attempt = tokio::task::spawn_blocking(move || {
        svc.agent_restart_observed_with_options(
            &tab_id,
            codeterm_service_core::service::agent_restart::RestartOptions {
                force: body.force,
                grace_secs: body.grace_secs,
                target_provider: body.provider,
                handoff_history: body.handoff_history,
                fresh: body.fresh,
            },
        )
    })
    .await;
    let attempt = match attempt {
        Ok(attempt) => attempt,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("restart worker failed: {error}"),
                    "restart_accepted": false,
                    "old_pty_torn_down": false,
                    "agent_ready": false,
                })),
            )
                .into_response();
        }
    };
    match attempt {
        Ok(outcome) => {
            (StatusCode::ACCEPTED, Json(agent_restart_response(&outcome))).into_response()
        }
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Failed {
            outcome,
            reason,
            timed_out,
        }) => (
            if timed_out {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            },
            Json(agent_restart_failure_response(&outcome, &reason)),
        )
            .into_response(),
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::NotFound(message),
        )) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": message})),
        )
            .into_response(),
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::BackgroundWork { items },
        )) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"reason":"background_work_running","running_items":items,"error":"restart refused while background work is running"})),
        ).into_response(),
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::RuntimeProviderHandoffUnsupported,
        )) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "reason": "runtime_provider_handoff_unsupported",
                "error": "Provider handoff into or out of an agent runtime is not supported; restart with the same provider",
            })),
        ).into_response(),
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::BadRequest(message),
        )) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": message})),
        )
            .into_response(),
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs },
        )) => {
            let mut headers = HeaderMap::new();
            headers.insert("Retry-After", retry_after_secs.to_string().parse().unwrap());
            (
                StatusCode::TOO_MANY_REQUESTS,
                headers,
                Json(serde_json::json!({
                    "error": "rate limited",
                    "retry_after_secs": retry_after_secs,
                })),
            )
                .into_response()
        }
        Err(codeterm_service_core::service::agent_restart::RestartAttemptError::Request(
            codeterm_service_core::service::agent_restart::AgentRestartError::Internal(message),
        )) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": message})),
        )
            .into_response(),
    }
}
