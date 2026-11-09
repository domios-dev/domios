use super::routes::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};

#[derive(Default, serde::Deserialize)]
pub(crate) struct StatusQuery {
    #[serde(default)]
    refresh: bool,
}

pub(crate) async fn status(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(provider): Path<String>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<codeterm_plugin::provider_cli::ProviderCliStatus>, (StatusCode, String)> {
    let registry = state.svc.provider_registry.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_cli::provider_cli_status_with_refresh(
            &registry,
            &provider,
            codeterm_service_core::agent::provider::current_platform_tag(),
            query.refresh,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

#[derive(serde::Deserialize)]
pub(crate) struct RunBody {
    operation: codeterm_service_core::service::provider_cli_actions::CliOperation,
    caller_tab_id: Option<String>,
}

pub(crate) async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(body): Json<RunBody>,
) -> Result<
    Json<codeterm_service_core::service::provider_cli_actions::CliLaunch>,
    (StatusCode, String),
> {
    let caller = crate::auth::caller_claim(&state, &headers, body.caller_tab_id.as_deref())?;
    let service = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_cli_actions::launch(
            &service,
            &provider,
            body.operation,
            caller,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
