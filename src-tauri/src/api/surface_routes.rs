//! REST surface for plugin surface tabs: list, open and close them, and stream or call a stream surface.

use crate::api::stream_census::{track, StreamGuard, StreamKind};
use axum::extract::{Path, Query, State as AxumState};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{http::HeaderMap, http::StatusCode, Json};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::routes::{check_auth, AppState};
use crate::service::browser::viewport::ViewportRequest;
use crate::service::surface::runtime::sse_payload;
use crate::service::surface::{
    call_surface, open_surface_stream, CallOrigin, SurfaceCallFailure, SurfaceError,
    SurfaceOpenRequest,
};
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
        SurfaceError::ViewUnavailable(_)
        | SurfaceError::Placement(_)
        | SurfaceError::NotAStreamSurface(_)
        | SurfaceError::RuntimeUnavailable(_) => StatusCode::CONFLICT,
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

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StreamQuery {
    #[serde(default)]
    w: Option<f64>,
    #[serde(default)]
    h: Option<f64>,
    #[serde(default)]
    dpr: Option<f64>,
}

/// The viewer's CSS size and DPR; absent when the viewer sent no size.
fn stream_viewport(query: &StreamQuery) -> Option<ViewportRequest> {
    Some(ViewportRequest {
        width: query.w?,
        height: query.h?,
        dpr: query.dpr.unwrap_or(1.0),
    })
}

/// SSE `state` / `frame` / `notice`; the stream is the viewer, so disconnecting leaves the screencast.
pub(crate) async fn stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, Refusal> {
    authorize(&state, &headers)?;
    let events = open_surface_stream(&state.svc, &tab_id, stream_viewport(&query))
        .await
        .map_err(surface_refusal)?;
    let events = events.map(|event| {
        let (name, data) = sse_payload(&event);
        Ok(Event::default().event(name).data(data))
    });
    Ok(
        Sse::new(track(events, StreamGuard::local(StreamKind::Surface)))
            .keep_alive(KeepAlive::default()),
    )
}

#[derive(Deserialize)]
pub(crate) struct SurfaceCallRequest {
    method: String,
    #[serde(default)]
    args: Value,
}

pub(crate) async fn call(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(request): Json<SurfaceCallRequest>,
) -> RouteResult {
    authorize(&state, &headers)?;
    let origin = if crate::api::tab_routes::caller_tab_header(&headers).is_some() {
        CallOrigin::Agent
    } else {
        CallOrigin::View
    };
    match call_surface(&state.svc, &tab_id, &request.method, request.args, origin).await {
        Ok(result) => Ok(Json(json!({ "result": result }))),
        Err(SurfaceCallFailure::Failed(error)) => Ok(Json(json!({ "error": error }))),
        Err(SurfaceCallFailure::Refused(error)) => Err(surface_refusal(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_stream_viewport_needs_width_and_height() {
        let query = StreamQuery {
            w: Some(1280.0),
            h: Some(720.0),
            dpr: None,
        };
        assert_eq!(
            stream_viewport(&query),
            Some(ViewportRequest {
                width: 1280.0,
                height: 720.0,
                dpr: 1.0
            })
        );
        assert_eq!(stream_viewport(&StreamQuery::default()), None);
    }

    #[test]
    fn surface_stream_refusals_are_conflicts_for_non_stream_tabs() {
        let (status, body) = surface_refusal(SurfaceError::NotAStreamSurface("t".into()));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.0["error"]["code"], "not_a_stream_surface");
        let (status, _) = surface_refusal(SurfaceError::TabNotFound("t".into()));
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
