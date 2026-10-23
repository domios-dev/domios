//! One browser-held SSE connection that carries many daemon streams as tagged channels.
//! HTTP/1.1 browsers cap a loopback origin at 6 sockets; per-stream fetches starved REST.

use crate::routes::AppState;
use axum::{
    body::{Body, Bytes},
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

mod frames;
#[cfg(test)]
mod tests;

use frames::{
    classify_channel_path, encode_end, encode_frame, encode_open, valid_mux_id, ChannelEnd,
    MuxTarget, SseSplitter, CHANNEL_FRAME_CAP,
};

const MAX_CHANNELS_PER_SESSION: usize = 256;
const SESSION_QUEUE: usize = 64;
const REFUSAL_BODY_LIMIT: usize = 64 * 1024;

struct Session {
    tx: mpsc::Sender<Bytes>,
    channels: HashMap<String, (u64, AbortHandle)>,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(Default::default);
static NEXT_CHANNEL_GENERATION: AtomicU64 = AtomicU64::new(1);

struct SessionGuard(String);

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = SESSIONS.lock().remove(&self.0) {
            for (_, handle) in session.channels.into_values() {
                handle.abort();
            }
        }
    }
}

#[derive(serde::Deserialize)]
pub struct MuxSessionQuery {
    pub session: String,
}

/// `GET /api/stream-mux?session=<id>` — the multiplexed SSE carrying every opened channel.
pub async fn stream_mux_open(
    Query(q): Query<MuxSessionQuery>,
) -> Result<Response, (StatusCode, String)> {
    if !valid_mux_id(&q.session) {
        return Err((StatusCode::BAD_REQUEST, "invalid mux session id".into()));
    }
    let (tx, mut rx) = mpsc::channel::<Bytes>(SESSION_QUEUE);
    {
        let mut sessions = SESSIONS.lock();
        if sessions.contains_key(&q.session) {
            return Err((StatusCode::CONFLICT, "mux session already open".into()));
        }
        sessions.insert(
            q.session.clone(),
            Session {
                tx,
                channels: HashMap::new(),
            },
        );
    }
    let guard = SessionGuard(q.session);
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b": mux\n\n"));
        while let Some(bytes) = rx.recv().await {
            yield Ok(bytes);
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

#[derive(serde::Deserialize)]
pub struct MuxChannelOpen {
    pub channel: String,
    pub path: String,
}

/// `POST /api/stream-mux/{session}/channels` — attach one stream as a channel of the session.
pub async fn stream_mux_open_channel(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(body): Json<MuxChannelOpen>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !valid_mux_id(&body.channel) {
        return Err((StatusCode::BAD_REQUEST, "invalid mux channel id".into()));
    }
    let target = classify_channel_path(&body.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| (StatusCode::GONE, "mux session is not open".into()))?;
    if !session.channels.contains_key(&body.channel)
        && session.channels.len() >= MAX_CHANNELS_PER_SESSION
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "mux session channel capacity reached".into(),
        ));
    }
    let generation = NEXT_CHANNEL_GENERATION.fetch_add(1, Ordering::Relaxed);
    let task = tokio::spawn(run_channel(
        state,
        headers,
        session_id.clone(),
        body.channel.clone(),
        generation,
        target,
        session.tx.clone(),
    ));
    if let Some((_, previous)) = session
        .channels
        .insert(body.channel, (generation, task.abort_handle()))
    {
        previous.abort();
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/stream-mux/{session}/channels/{channel}` — idempotent channel release.
pub async fn stream_mux_close_channel(
    AxumPath((session_id, channel)): AxumPath<(String, String)>,
) -> Json<serde_json::Value> {
    let removed = SESSIONS
        .lock()
        .get_mut(&session_id)
        .and_then(|session| session.channels.remove(&channel));
    let closed = removed.is_some();
    if let Some((_, handle)) = removed {
        handle.abort();
    }
    Json(serde_json::json!({ "ok": true, "closed": closed }))
}

async fn run_channel(
    state: AppState,
    headers: HeaderMap,
    session_id: String,
    channel: String,
    generation: u64,
    target: MuxTarget,
    tx: mpsc::Sender<Bytes>,
) {
    if let Some(end) = pump_channel(state, headers, &channel, target, &tx).await {
        let _ = tx.send(encode_end(&channel, &end)).await;
    }
    let mut sessions = SESSIONS.lock();
    if let Some(session) = sessions.get_mut(&session_id) {
        if session.channels.get(&channel).map(|(g, _)| *g) == Some(generation) {
            session.channels.remove(&channel);
        }
    }
}

async fn open_target(
    state: AppState,
    headers: HeaderMap,
    target: MuxTarget,
) -> Result<Response, (StatusCode, String)> {
    let bad_query = |error: axum::extract::rejection::QueryRejection| {
        (StatusCode::BAD_REQUEST, error.body_text())
    };
    match target {
        MuxTarget::Relay(uri) => {
            let query = Query::try_from_uri(&uri).map_err(bad_query)?;
            crate::mesh_routes::mesh_relay_stream(AxumState(state), headers, query).await
        }
        MuxTarget::LocalOutput(uri) => {
            let query = Query::try_from_uri(&uri).map_err(bad_query)?;
            crate::output_stream::output_stream(AxumState(state), headers, query)
                .await
                .map(IntoResponse::into_response)
        }
    }
}

/// `None` means the session receiver is gone, so no end frame can be delivered.
async fn pump_channel(
    state: AppState,
    headers: HeaderMap,
    channel: &str,
    target: MuxTarget,
    tx: &mpsc::Sender<Bytes>,
) -> Option<ChannelEnd> {
    let response = match open_target(state, headers, target).await {
        Ok(response) => response,
        Err((status, body)) => {
            return Some(ChannelEnd::Http {
                status: status.as_u16(),
                body,
            })
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = axum::body::to_bytes(response.into_body(), REFUSAL_BODY_LIMIT)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        return Some(ChannelEnd::Http {
            status: status.as_u16(),
            body,
        });
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    tx.send(encode_open(channel, content_type.as_deref()))
        .await
        .ok()?;
    let mut upstream = response.into_body().into_data_stream();
    let mut splitter = SseSplitter::new(CHANNEL_FRAME_CAP);
    while let Some(chunk) = upstream.next().await {
        let bytes = match chunk {
            Ok(bytes) => bytes,
            Err(error) => {
                return Some(ChannelEnd::Error {
                    message: error.to_string(),
                })
            }
        };
        let frames = match splitter.push(&bytes) {
            Ok(frames) => frames,
            Err(bytes) => return Some(ChannelEnd::Capacity { bytes }),
        };
        for frame in frames {
            tx.send(encode_frame(channel, &frame)).await.ok()?;
        }
    }
    Some(ChannelEnd::Eof)
}
