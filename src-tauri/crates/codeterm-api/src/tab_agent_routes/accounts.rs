use crate::routes::AppState;
use axum::http::HeaderMap;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::provider_accounts;

pub(super) async fn list(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<provider_accounts::roster::Snapshot>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || provider_accounts::roster::snapshot(&state.svc, &provider))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The account list is unavailable".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub(super) async fn update(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(request): Json<provider_accounts::Request>,
) -> Result<Json<provider_accounts::roster::Snapshot>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || provider_accounts::update(&state.svc, &provider, request))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The account change could not finish".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub(super) async fn tab_status(
    State(state): State<AppState>,
    Path(tab): Path<String>,
) -> Result<Json<Option<provider_accounts::Transition>>, (StatusCode, String)> {
    if state.svc.mux.read().get_tab(&tab).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            "The account tab is unavailable".into(),
        ));
    }
    Ok(Json(provider_accounts::transition(&state.svc, &tab)))
}

pub(super) async fn select(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<provider_accounts::SelectRequest>,
) -> Result<Json<provider_accounts::Transition>, (StatusCode, String)> {
    crate::runtime_login::require_permission_authority(&state, &headers)?;
    select_inner(state, tab, request, false).await
}

pub(super) async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<provider_accounts::SelectRequest>,
) -> Result<Json<provider_accounts::Transition>, (StatusCode, String)> {
    crate::runtime_login::require_permission_authority(&state, &headers)?;
    select_inner(state, tab, request, true).await
}

async fn select_inner(
    state: AppState,
    tab: String,
    request: provider_accounts::SelectRequest,
    verify: bool,
) -> Result<Json<provider_accounts::Transition>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        provider_accounts::select(&state.svc, &tab, request, verify)
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "The account switch could not finish".into(),
        )
    })?
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RefreshRequest {
    nonce: String,
    epoch: Option<u64>,
}

pub(super) async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<RefreshRequest>,
) -> Result<Json<Option<provider_accounts::Transition>>, (StatusCode, String)> {
    crate::runtime_login::require_permission_authority(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        provider_accounts::refresh_login(&state.svc, &tab, &request.nonce, request.epoch)
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "The saved sign-in could not be applied".into(),
        )
    })?
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}
