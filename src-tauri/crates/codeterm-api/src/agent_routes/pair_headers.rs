use axum::{extract::State as AxumState, http::HeaderMap, Json};

use crate::routes::{check_auth, AppState};

/// Paired creation (task_7e5f9354): REST identity comes from authenticated caller context.
pub(crate) async fn create_orchestration_pair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<codeterm_service_core::service::OrchestrationPairParams>,
) -> Result<Json<serde_json::Value>, super::SpawnRejection> {
    check_auth(&state, &headers).map_err(|(status, message)| {
        super::SpawnRejection::new(status, "unauthorized", message.to_string())
    })?;
    let params = pair_params_from_headers(&headers, body);
    let probe = params.clone();
    super::spawn_rejection::validate_spawn_cwd(
        &state.svc,
        probe.workspace_id.as_deref(),
        probe.spawned_by_tab_id.as_deref(),
        None,
    )
    .await?;
    let result = match state.svc.create_orchestration_pair(params).await {
        Ok(result) => result,
        Err(error) => {
            super::spawn_rejection::validate_spawn_cwd(
                &state.svc,
                probe.workspace_id.as_deref(),
                probe.spawned_by_tab_id.as_deref(),
                None,
            )
            .await?;
            return Err(super::SpawnRejection::from(error));
        }
    };
    Ok(Json(serde_json::to_value(result).map_err(|error| {
        super::SpawnRejection::from(error.to_string())
    })?))
}

pub(super) fn pair_params_from_headers(
    headers: &HeaderMap,
    mut params: codeterm_service_core::service::OrchestrationPairParams,
) -> codeterm_service_core::service::OrchestrationPairParams {
    params.spawned_by_tab_id = crate::tab_routes::caller_tab_header(headers);
    params
}
