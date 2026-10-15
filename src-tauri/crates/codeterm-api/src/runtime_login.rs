use super::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::agent_runtime::login::{operate, LoginRequest};

pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    if request.api_key.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "API keys must be entered on the provider's own machine".into(),
        ));
    }
    operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}
