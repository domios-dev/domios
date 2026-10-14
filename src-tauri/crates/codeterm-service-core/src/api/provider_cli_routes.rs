use super::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};

pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> Result<Json<codeterm_plugin::provider_cli::ProviderCliStatus>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(code, message)| (code, message.to_owned()))?;
    let registry = state.svc.provider_registry.clone();
    tokio::task::spawn_blocking(move || {
        crate::service::provider_cli::provider_cli_status(
            &registry,
            &provider,
            crate::agent::provider::current_platform_tag(),
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

#[derive(serde::Deserialize)]
pub(crate) struct RunBody {
    operation: crate::service::provider_cli_actions::CliOperation,
    caller_tab_id: Option<String>,
}

pub(crate) async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(body): Json<RunBody>,
) -> Result<Json<crate::service::provider_cli_actions::CliLaunch>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(code, message)| (code, message.to_owned()))?;
    let caller = crate::api::tab_routes::caller_tab_header(&headers).or(body.caller_tab_id);
    let service = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        crate::service::provider_cli_actions::launch(&service, &provider, body.operation, caller)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
