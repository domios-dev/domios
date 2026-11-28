use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use codeterm_service_core::service::UTILITY_TEAM_REFUSAL;

use super::{create_shell_tab, AppState, CreateShellTabBody};

pub async fn create_shell_tab_response(
    state: AxumState<AppState>,
    headers: HeaderMap,
    body: Json<CreateShellTabBody>,
) -> Response {
    match create_shell_tab(state, headers, body).await {
        Ok(value) => value.into_response(),
        Err((status, detail)) => {
            if let Some((code, message)) = detail.split_once(": ") {
                if code == UTILITY_TEAM_REFUSAL {
                    return (
                        status,
                        Json(serde_json::json!({ "code": code, "message": message })),
                    )
                        .into_response();
                }
            }
            (status, detail).into_response()
        }
    }
}

pub(super) fn create_shell_error_status(error: &str) -> StatusCode {
    if error.split_once(':').is_some_and(|(code, _)| {
        matches!(
            code,
            "neighbor_capability_denied"
                | "neighbor_forbidden"
                | "neighbor_paused"
                | "tab_requests_not_allowed"
                | "neighbor_machine_access_unsupported"
                | UTILITY_TEAM_REFUSAL
        )
    }) {
        return StatusCode::FORBIDDEN;
    }
    if error.starts_with("utility_tab_limit:") {
        StatusCode::CONFLICT
    } else if error.starts_with("Unknown Lucide icon") || error.starts_with("timeout requires ") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
