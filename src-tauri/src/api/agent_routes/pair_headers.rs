use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

/// Paired creation (task_7e5f9354): REST identity comes from authenticated caller context.
pub(super) async fn create_orchestration_pair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<crate::service::OrchestrationPairParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let params = pair_params_from_headers(&headers, body);
    let result = state
        .svc
        .create_orchestration_pair(params)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::to_value(result).map_err(|error| {
        (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    })?))
}

pub(super) fn pair_params_from_headers(
    headers: &HeaderMap,
    mut params: crate::service::OrchestrationPairParams,
) -> crate::service::OrchestrationPairParams {
    params.spawned_by_tab_id = crate::api::tab_routes::caller_tab_header(headers);
    params
}
