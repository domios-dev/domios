use std::net::SocketAddr;

use axum::{
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State},
    http::{header, uri::Authority, HeaderMap, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use codeterm_service_core::service::dictation::live_capture::{
    LiveCaptureAdmission, LiveCaptureStart, LiveCaptureUpdate, PauseReason,
};
use codeterm_types::dictation::MAX_PCM_BYTES;
use serde::Deserialize;
use uuid::Uuid;

use crate::plugin_host_action_routes::owner_post;
use crate::{
    auth::{AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

type Refusal = (StatusCode, String);

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/start",
            owner_post(start).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/{lease}/frame/{sequence}",
            owner_post(frame).layer(DefaultBodyLimit::max(MAX_PCM_BYTES)),
        )
        .route(
            "/{lease}/pause/{sequence}",
            owner_post(pause).layer(DefaultBodyLimit::max(256)),
        )
        .route("/{lease}/cancel", owner_post(cancel))
        .route_layer(middleware::from_fn(require_direct_secure_owner))
}

async fn require_direct_secure_owner(request: Request, next: Next) -> Response {
    let owner = matches!(
        request.extensions().get::<CallerIdentity>(),
        Some(CallerIdentity::Local)
    ) && matches!(
        request.extensions().get::<AuthenticatedOrigin>(),
        Some(AuthenticatedOrigin::Owner)
    );
    if !owner || !secure_capture_origin(&request) {
        return (
            StatusCode::FORBIDDEN,
            "Live capture needs a direct secure owner session.",
        )
            .into_response();
    }
    next.run(request).await
}

fn secure_capture_origin(request: &Request) -> bool {
    let local = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().to_canonical().is_loopback());
    let Some(origin) = request.headers().get(header::ORIGIN) else {
        return local;
    };
    let Ok(origin) = origin.to_str().unwrap_or_default().parse::<Uri>() else {
        return false;
    };
    let Some(authority) = origin.authority() else {
        return false;
    };
    if origin.path() != "/" || origin.query().is_some() || authority.as_str().contains('@') {
        return false;
    }
    let scheme = origin.scheme_str();
    if local
        && matches!(
            authority.host(),
            "localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost" | "localhost.tauri"
        )
        && matches!(scheme, Some("https" | "http" | "tauri"))
    {
        return true;
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| host.parse::<Authority>().ok())
        .or_else(|| request.uri().authority().cloned());
    scheme == Some("https") && host.as_ref() == Some(authority)
}

fn client(headers: &HeaderMap) -> Result<Uuid, Refusal> {
    headers
        .get("x-codeterm-client-id")
        .and_then(|header| header.to_str().ok())
        .and_then(|value| {
            Uuid::parse_str(value)
                .ok()
                .filter(|id| id.to_string() == value)
        })
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Invalid live capture client identity.".into(),
        ))
}

async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LiveCaptureStart>,
) -> Result<Json<LiveCaptureAdmission>, Refusal> {
    if request.tab_id.is_empty() || request.tab_id.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid live capture tab identity.".into(),
        ));
    }
    Ok(Json(
        state
            .svc
            .start_live_capture(client(&headers)?, request)
            .await,
    ))
}

async fn frame(
    State(state): State<AppState>,
    Path((lease, sequence)): Path<(Uuid, u32)>,
    headers: HeaderMap,
    audio: Bytes,
) -> Result<Json<LiveCaptureUpdate>, Refusal> {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/octet-stream")
    {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected little-endian 16-bit PCM.".into(),
        ));
    }
    state
        .svc
        .live_capture_sessions()
        .frame(client(&headers)?, lease, sequence, audio.to_vec())
        .await
        .map(Json)
        .map_err(conflict)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PauseRequest {
    reason: PauseReason,
}

async fn pause(
    State(state): State<AppState>,
    Path((lease, sequence)): Path<(Uuid, u32)>,
    headers: HeaderMap,
    Json(request): Json<PauseRequest>,
) -> Result<Json<LiveCaptureUpdate>, Refusal> {
    state
        .svc
        .live_capture_sessions()
        .pause(client(&headers)?, lease, sequence, request.reason)
        .await
        .map(Json)
        .map_err(conflict)
}

async fn cancel(
    State(state): State<AppState>,
    Path(lease): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, Refusal> {
    state
        .svc
        .live_capture_sessions()
        .cancel(client(&headers)?, lease);
    Ok(StatusCode::NO_CONTENT)
}

fn conflict(reason: String) -> Refusal {
    (StatusCode::CONFLICT, reason)
}
