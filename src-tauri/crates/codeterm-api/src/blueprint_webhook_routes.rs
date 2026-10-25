use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_sentinel_host::{
    blueprint_webhook::{self, Refusal},
    SentinelExt,
};
use serde::Deserialize;
use serde_json::Value;

use crate::routes::AppState;

pub(crate) fn is_ingress(path: &str) -> bool {
    path.strip_prefix("/api/blueprints/")
        .and_then(|path| path.strip_suffix("/webhook"))
        .is_some_and(|id| !id.is_empty() && id.len() <= 128 && !id.contains('/'))
}

fn refusal(error: Refusal) -> (StatusCode, &'static str) {
    match error {
        Refusal::Unavailable => (StatusCode::NOT_FOUND, "webhook unavailable"),
        Refusal::Unauthorized => (StatusCode::UNAUTHORIZED, "webhook authentication failed"),
        Refusal::InvalidPayload => (StatusCode::BAD_REQUEST, "invalid webhook payload"),
        Refusal::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "webhook rate limit exceeded"),
        Refusal::Busy => (StatusCode::CONFLICT, "webhook busy"),
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

pub(crate) async fn receive(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    let hooks = blueprint_webhook::service(&state.svc).map_err(refusal)?;
    hooks
        .before_read(chrono::Utc::now().timestamp_millis().max(0) as u64)
        .map_err(refusal)?;
    let bearer = header(&headers, "authorization")
        .and_then(|s| s.strip_prefix("Bearer ").map(str::to_owned));
    let signature = header(&headers, "x-hub-signature-256");
    if bearer.is_none() && signature.is_none() {
        return Err(refusal(Refusal::Unauthorized));
    }
    let content_type = header(&headers, "content-type").unwrap_or_default();
    if !content_type
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "webhook requires application/json",
        ));
    }
    let bytes = axum::body::to_bytes(body, blueprint_webhook::MAX_BODY_BYTES)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "webhook body exceeds 1 MiB"))?;
    hooks
        .receive(
            state.svc,
            id,
            blueprint_webhook::Authentication {
                bearer,
                signature,
                timestamp: header(&headers, "x-webhook-timestamp"),
            },
            header(&headers, "x-delivery-id").or_else(|| header(&headers, "x-github-delivery")),
            bytes.to_vec(),
        )
        .await
        .map(Json)
        .map_err(refusal)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configure {
    caller_tab_id: String,
    session_binding_nonce: Option<String>,
    action: String,
    #[serde(default)]
    reveal: bool,
}

pub(crate) async fn configure(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Configure>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    crate::auth::caller_access_level(&state, &headers)
        .map_err(|_| (StatusCode::FORBIDDEN, "webhook management refused"))?;
    if crate::auth::caller_peer_id(&state, &headers).is_some() {
        return Err((StatusCode::FORBIDDEN, "webhook management is local only"));
    }
    let caller = state
        .svc
        .sentinel_caller(&body.caller_tab_id, body.session_binding_nonce.as_deref())
        .map_err(|_| (StatusCode::FORBIDDEN, "webhook management refused"))?;
    blueprint_webhook::service(&state.svc)
        .map_err(refusal)?
        .configure(&state.svc, &caller, &id, &body.action, body.reveal)
        .await
        .map(Json)
        .map_err(|_| (StatusCode::BAD_REQUEST, "webhook configuration refused"))
}

#[cfg(test)]
mod tests;
