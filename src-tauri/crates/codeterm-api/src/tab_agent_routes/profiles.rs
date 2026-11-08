use crate::routes::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::agent_runtime::launch_profile_install as profiles;

pub(super) mod registry;

pub(super) async fn list(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<Vec<profiles::ProfileState>>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || profiles::catalog(&state.svc, &provider))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Profile catalog unavailable".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SetupRequest {
    profile_id: String,
    confirmed: bool,
    #[serde(default)]
    command: Option<codeterm_plugin::agent_runtime::AgentRuntimeCommand>,
}
pub(super) async fn setup(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(request): Json<SetupRequest>,
) -> Result<
    (
        StatusCode,
        Json<codeterm_service_core::service::agent_runtime::launch_profile_setup::SetupJob>,
    ),
    (StatusCode, String),
> {
    if !request.confirmed {
        return Err((
            StatusCode::BAD_REQUEST,
            "Explicit owner confirmation required".into(),
        ));
    }
    let result = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::agent_runtime::launch_profile_setup::begin(
            &state.svc,
            provider,
            request.profile_id,
            request.command,
        )
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Profile setup unavailable".into(),
        )
    })?;
    result
        .map(|job| (StatusCode::ACCEPTED, Json(job)))
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
