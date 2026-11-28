use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    middleware, Json, Router,
};
use codeterm_service_core::service::dictation::{DictationFrameResult, DictationSessions};
use codeterm_types::dictation::{DictationStart, DictationStartResult, MAX_PCM_BYTES};

use super::live_transcript_capture::require_direct_secure_owner;
use crate::plugin_host_action_routes::owner_post;
use crate::routes::AppState;

type Refusal = (StatusCode, String);

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/dictation/start",
            owner_post(start).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/dictation/{session}/frame/{sequence}",
            owner_post(frame).layer(DefaultBodyLimit::max(MAX_PCM_BYTES)),
        )
        .route(
            "/api/dictation/{session}/finish/{sequence}",
            owner_post(finish),
        )
        .route("/api/dictation/{session}/cancel", owner_post(cancel))
        .route_layer(middleware::from_fn(require_direct_secure_owner))
}

fn client(headers: &HeaderMap, session: &str) -> Result<String, Refusal> {
    let valid = |input: &str| {
        uuid::Uuid::parse_str(input).is_ok_and(|id| id.hyphenated().to_string() == input)
    };
    match headers
        .get("x-codeterm-client-id")
        .and_then(|value| value.to_str().ok())
    {
        Some(client) if valid(client) && valid(session) => Ok(client.into()),
        _ => Err((
            StatusCode::BAD_REQUEST,
            "Invalid dictation session or frontend identity.".into(),
        )),
    }
}

async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DictationStart>,
) -> Result<Json<DictationStartResult>, Refusal> {
    let client = client(&headers, &request.session)?;
    Ok(Json(state.svc.start_dictation(&client, request).await))
}

async fn frame(
    State(state): State<AppState>,
    Path((session, sequence)): Path<(String, u32)>,
    headers: HeaderMap,
    audio: Bytes,
) -> Result<Json<DictationFrameResult>, Refusal> {
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
    submit(&state, &headers, &session, sequence, audio.to_vec(), false).await
}

async fn finish(
    State(state): State<AppState>,
    Path((session, sequence)): Path<(String, u32)>,
    headers: HeaderMap,
) -> Result<Json<DictationFrameResult>, Refusal> {
    submit(&state, &headers, &session, sequence, Vec::new(), true).await
}

async fn submit(
    state: &AppState,
    headers: &HeaderMap,
    session: &str,
    sequence: u32,
    pcm: Vec<u8>,
    finish: bool,
) -> Result<Json<DictationFrameResult>, Refusal> {
    let client = client(headers, session)?;
    state
        .svc
        .ext::<DictationSessions>()
        .frame(&client, session, sequence, pcm, finish)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

async fn cancel(
    State(state): State<AppState>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Refusal> {
    let client = client(&headers, &session)?;
    state
        .svc
        .ext::<DictationSessions>()
        .cancel(&client, &session);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
