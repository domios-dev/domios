use axum::{extract::State as AxumState, http::HeaderMap, Json};

use crate::api::routes::{check_auth, AppState};

/// Paired creation (task_7e5f9354): REST identity comes from authenticated caller context.
pub(crate) async fn create_orchestration_pair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<crate::service::OrchestrationPairParams>,
) -> Result<Json<serde_json::Value>, super::SpawnRejection> {
    check_auth(&state, &headers).map_err(|(status, message)| {
        super::SpawnRejection::new(status, "unauthorized", message.to_string())
    })?;
    let params = pair_params_from_headers(&headers, body);
    let svc = state.svc.clone();
    let probe = params.clone();
    tokio::task::spawn_blocking(move || {
        svc.validate_request_cwd(
            probe.workspace_id.as_deref(),
            probe.spawned_by_tab_id.as_deref(),
            None,
        )
    })
    .await
    .map_err(|error| super::SpawnRejection::from(error.to_string()))?
    .map_err(super::SpawnRejection::from)?;
    let result = state
        .svc
        .create_orchestration_pair(params)
        .await
        .map_err(super::SpawnRejection::from)?;
    Ok(Json(serde_json::to_value(result).map_err(|error| {
        super::SpawnRejection::from(error.to_string())
    })?))
}

pub(super) fn pair_params_from_headers(
    headers: &HeaderMap,
    mut params: crate::service::OrchestrationPairParams,
) -> crate::service::OrchestrationPairParams {
    params.spawned_by_tab_id = crate::api::tab_routes::caller_tab_header(headers);
    params
}
