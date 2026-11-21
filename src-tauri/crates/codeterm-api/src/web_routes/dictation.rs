use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::dictation::DictationSessions;
use codeterm_types::dictation::{DictationStart, DictationStartResult, DictationUpdate};

use crate::routes::AppState;

type Refusal = (StatusCode, String);

fn client(headers: &HeaderMap, session: &str) -> Result<String, Refusal> {
    let valid = |input: &str| {
        uuid::Uuid::parse_str(input).is_ok_and(|id| id.hyphenated().to_string() == input)
    };
    match headers.get("x-codeterm-client-id").and_then(|value| value.to_str().ok()) {
        Some(client) if valid(client) && valid(session) => Ok(client.into()),
        _ => Err((StatusCode::BAD_REQUEST, "Invalid dictation session or frontend identity.".into())),
    }
}

pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DictationStart>,
) -> Result<Json<DictationStartResult>, Refusal> {
    let client = client(&headers, &request.session)?;
    Ok(Json(state.svc.start_dictation(&client, request).await))
}

pub async fn frame(
    State(state): State<AppState>,
    Path((session, sequence)): Path<(String, u32)>,
    headers: HeaderMap,
    audio: Bytes,
) -> Result<Json<DictationUpdate>, Refusal> {
    if headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()) != Some("application/octet-stream") {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "Expected little-endian 16-bit PCM.".into()));
    }
    submit(&state, &headers, &session, sequence, audio.to_vec(), false).await
}

pub async fn finish(
    State(state): State<AppState>,
    Path((session, sequence)): Path<(String, u32)>,
    headers: HeaderMap,
) -> Result<Json<DictationUpdate>, Refusal> {
    submit(&state, &headers, &session, sequence, Vec::new(), true).await
}

async fn submit(
    state: &AppState,
    headers: &HeaderMap,
    session: &str,
    sequence: u32,
    pcm: Vec<u8>,
    finish: bool,
) -> Result<Json<DictationUpdate>, Refusal> {
    let client = client(headers, session)?;
    state.svc.ext::<DictationSessions>().frame(&client, session, sequence, pcm, finish).await
        .map(Json).map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub async fn cancel(
    State(state): State<AppState>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Refusal> {
    let client = client(&headers, &session)?;
    state.svc.ext::<DictationSessions>().cancel(&client, &session);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
