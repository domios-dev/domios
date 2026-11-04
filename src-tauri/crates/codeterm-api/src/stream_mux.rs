use axum::{
    body::Body,
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    },
};
use tokio::task::AbortHandle;

pub(crate) mod dispatch;
mod frames;
mod paths;
mod rpc;
mod scheduler;
mod spool;
#[cfg(test)]
mod tests;

use scheduler::{Scheduler, Traffic};
type Failure = (StatusCode, String);

pub(crate) fn router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {
    axum::Router::new()
        .route("/api/stream-mux", axum::routing::get(stream_mux_open))
        .route(
            "/api/stream-mux/{session}/channels",
            axum::routing::post(stream_mux_open_channel),
        )
        .route(
            "/api/stream-mux/{session}/channels/{channel}",
            axum::routing::delete(stream_mux_close_channel),
        )
        .route(
            "/api/stream-mux/{session}/credit",
            axum::routing::post(stream_mux_credit),
        )
        .route(
            "/api/stream-mux/{session}/requests/{id}",
            axum::routing::post(stream_mux_request)
                .delete(stream_mux_cancel_request)
                .layer(axum::extract::DefaultBodyLimit::disable()),
        )
}

struct Entry {
    stream: bool,
    traffic: Traffic,
    cancellable: AtomicBool,
    detached: AtomicBool,
    terminal_queued: AtomicBool,
    task: Mutex<Option<AbortHandle>>,
}

struct Session {
    owner: HeaderValue,
    scheduler: Scheduler,
    entries: HashMap<String, Arc<Entry>>,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(Default::default);

struct SessionGuard(String);
impl Drop for SessionGuard {
    fn drop(&mut self) {
        let removed = { SESSIONS.lock().remove(&self.0) };
        if let Some(session) = removed {
            session.scheduler.close();
            for entry in session.entries.into_values() {
                entry.detached.store(true, Ordering::Release);
                if entry.cancellable.load(Ordering::Acquire) {
                    if let Some(task) = entry.task.lock().as_ref() {
                        task.abort();
                    }
                }
            }
        }
    }
}

struct Reservation {
    session: String,
    id: String,
    entry: Arc<Entry>,
    scheduler: Scheduler,
    keep_queue: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        {
            let mut sessions = SESSIONS.lock();
            if let Some(session) = sessions.get_mut(&self.session) {
                if session
                    .entries
                    .get(&self.id)
                    .is_some_and(|entry| Arc::ptr_eq(entry, &self.entry))
                {
                    session.entries.remove(&self.id);
                }
            }
        }
        if !self.keep_queue {
            self.scheduler.remove(&self.id);
        }
    }
}

fn owner(headers: &HeaderMap) -> Result<HeaderValue, Failure> {
    headers
        .get("x-codeterm-client-id")
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Mux client id required".into()))
}

fn reserve(
    session_id: &str,
    id: &str,
    headers: &HeaderMap,
    traffic: Traffic,
    stream: bool,
) -> Result<Reservation, Failure> {
    if !frames::valid_mux_id(id) {
        return Err((StatusCode::BAD_REQUEST, "Invalid mux request id".into()));
    }
    let client = owner(headers)?;
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| (StatusCode::GONE, "Mux session is closed".into()))?;
    let streams = session
        .entries
        .values()
        .filter(|entry| entry.stream)
        .count();
    let requests = session.entries.len() - streams;
    let bulk = session
        .entries
        .values()
        .filter(|entry| !entry.stream && entry.traffic == Traffic::Bulk)
        .count();
    rpc::model::admission(
        session.owner == client,
        session.entries.contains_key(id),
        stream,
        streams,
        requests,
        bulk,
        traffic == Traffic::Bulk,
    )
    .map_err(|status| {
        (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            "Mux admission refused".into(),
        )
    })?;
    if !session.scheduler.register(id.to_owned(), traffic, stream) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Mux response capacity reached".into(),
        ));
    }
    let entry = Arc::new(Entry {
        stream,
        traffic,
        cancellable: AtomicBool::new(stream),
        detached: AtomicBool::new(false),
        terminal_queued: AtomicBool::new(false),
        task: Mutex::new(None),
    });
    session.entries.insert(id.to_owned(), entry.clone());
    Ok(Reservation {
        session: session_id.to_owned(),
        id: id.to_owned(),
        entry,
        scheduler: session.scheduler.clone(),
        keep_queue: false,
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct MuxSessionQuery {
    pub session: String,
}

pub(crate) async fn stream_mux_open(
    headers: HeaderMap,
    Query(query): Query<MuxSessionQuery>,
) -> Result<Response, Failure> {
    if !frames::valid_mux_id(&query.session) {
        return Err((StatusCode::BAD_REQUEST, "Invalid mux session id".into()));
    }
    let owner = owner(&headers)?;
    let scheduler = Scheduler::new();
    {
        let mut sessions = SESSIONS.lock();
        if sessions.contains_key(&query.session) {
            return Err((StatusCode::CONFLICT, "Mux session already open".into()));
        }
        sessions.insert(
            query.session.clone(),
            Session {
                owner,
                scheduler: scheduler.clone(),
                entries: HashMap::new(),
            },
        );
    }
    let guard = SessionGuard(query.session);
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(b": mux\n\n"));
        while let Some(bytes) = scheduler.recv().await { yield Ok(bytes); }
    };
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

#[derive(serde::Deserialize)]
pub(crate) struct MuxChannelOpen {
    pub channel: String,
    pub path: String,
}

pub(crate) use rpc::{stream_mux_open_channel, stream_mux_request};

async fn cancel(
    session_id: String,
    id: String,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Failure> {
    let owner = owner(&headers)?;
    let target = {
        let sessions = SESSIONS.lock();
        match sessions.get(&session_id) {
            Some(session) if session.owner != owner => {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Mux client differs from session owner".into(),
                ))
            }
            Some(session) => session
                .entries
                .get(&id)
                .map(|entry| (entry.clone(), session.scheduler.clone())),
            None => None,
        }
    };
    let closed = target.is_some();
    if let Some((entry, scheduler)) = target {
        entry.detached.store(true, Ordering::Release);
        scheduler.remove(&id);
        if entry.cancellable.load(Ordering::Acquire) {
            if let Some(task) = entry.task.lock().as_ref() {
                task.abort();
            }
        }
    }
    Ok(Json(serde_json::json!({"accepted":true,"closed":closed})))
}

#[derive(serde::Deserialize)]
pub(crate) struct Credit {
    received: u64,
}

/// The client acknowledges cumulative credited bulk frames so the writer can release more bulk.
pub(crate) async fn stream_mux_credit(
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(credit): Json<Credit>,
) -> Result<StatusCode, Failure> {
    let owner = owner(&headers)?;
    let scheduler = {
        let sessions = SESSIONS.lock();
        match sessions.get(&session_id) {
            Some(session) if session.owner != owner => {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Mux client differs from session owner".into(),
                ))
            }
            Some(session) => session.scheduler.clone(),
            None => return Err((StatusCode::GONE, "Unknown mux session".into())),
        }
    };
    scheduler.credit(credit.received);
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn stream_mux_close_channel(
    headers: HeaderMap,
    Path((session, channel)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, Failure> {
    cancel(session, channel, headers).await
}
pub(crate) async fn stream_mux_cancel_request(
    headers: HeaderMap,
    Path((session, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, Failure> {
    cancel(session, id, headers).await
}
