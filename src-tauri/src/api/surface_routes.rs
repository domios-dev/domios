//! REST surface for plugin surface tabs: list declarations and open tabs, open one, close one.

use axum::{extract::State as AxumState, http::HeaderMap, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::routes::{check_auth, AppState};
use crate::service::surface::{SurfaceError, SurfaceOpenRequest};
use crate::service::{ServiceContext, ServiceState};

type Refusal = (StatusCode, Json<Value>);
type RouteResult = Result<Json<Value>, Refusal>;

fn refusal(status: StatusCode, code: &str, message: &str) -> Refusal {
    (
        status,
        Json(json!({ "ok": false, "error": { "code": code, "message": message } })),
    )
}

fn surface_refusal(error: SurfaceError) -> Refusal {
    let status = match error {
        SurfaceError::InvalidTarget(_)
        | SurfaceError::OwnerIsSurface(_)
        | SurfaceError::WorkspaceMismatch { .. }
        | SurfaceError::NotASurface(_) => StatusCode::BAD_REQUEST,
        SurfaceError::PluginUnavailable(_)
        | SurfaceError::SurfaceNotDeclared { .. }
        | SurfaceError::OwnerNotFound(_)
        | SurfaceError::TabNotFound(_) => StatusCode::NOT_FOUND,
        SurfaceError::ViewUnavailable(_) | SurfaceError::Placement(_) => StatusCode::CONFLICT,
    };
    refusal(status, error.code(), &error.message())
}

fn internal() -> Refusal {
    refusal(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Surface request did not complete",
    )
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Refusal> {
    check_auth(state, headers).map_err(|(status, message)| refusal(status, "unauthorized", message))
}

async fn on_blocking_pool<T: Serialize + Send + 'static>(
    svc: ServiceState,
    work: impl FnOnce(&ServiceContext) -> Result<T, SurfaceError> + Send + 'static,
) -> RouteResult {
    let result = tokio::task::spawn_blocking(move || work(svc.as_ref()))
        .await
        .map_err(|_| internal())?
        .map_err(surface_refusal)?;
    serde_json::to_value(result)
        .map(Json)
        .map_err(|_| internal())
}

pub(crate) async fn list(AxumState(state): AxumState<AppState>, headers: HeaderMap) -> RouteResult {
    authorize(&state, &headers)?;
    on_blocking_pool(state.svc.clone(), |svc| Ok(svc.list_surfaces())).await
}

pub(crate) async fn open(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceOpenRequest>,
) -> RouteResult {
    authorize(&state, &headers)?;
    on_blocking_pool(state.svc.clone(), move |svc| svc.open_surface(request)).await
}

#[derive(Deserialize)]
pub(crate) struct SurfaceCloseRequest {
    tab_id: String,
    #[serde(default)]
    caller_tab_id: Option<String>,
}

pub(crate) async fn close(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceCloseRequest>,
) -> RouteResult {
    authorize(&state, &headers)?;
    on_blocking_pool(state.svc.clone(), move |svc| {
        svc.close_surface(request.caller_tab_id.as_deref(), &request.tab_id)
    })
    .await
}
