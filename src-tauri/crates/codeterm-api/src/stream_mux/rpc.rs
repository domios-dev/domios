use super::{
    dispatch::{self, Context, Dispatcher},
    frames::{self, ChannelEnd},
    scheduler::{Kind, Push, Traffic},
    spool, Failure, MuxChannelOpen, Reservation,
};
use axum::{
    body::Body,
    extract::{Path, Request},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    Extension, Json,
};
use futures_util::StreamExt;
use std::sync::atomic::Ordering;

pub(crate) mod model;

type Ack = (StatusCode, Json<serde_json::Value>);
fn ack() -> Ack {
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"accepted":true})),
    )
}

pub(crate) async fn stream_mux_open_channel(
    Extension(dispatcher): Extension<Dispatcher>,
    Path(session): Path<String>,
    request: Request,
) -> Result<Ack, Failure> {
    let context = Context::from_request(&request);
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let channel: MuxChannelOpen =
        serde_json::from_slice(&bytes).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let uri =
        frames::classify_channel_path(&channel.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    model::validate_path(&channel.path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let reservation = super::reserve(
        &session,
        &channel.channel,
        &parts.headers,
        Traffic::Bulk,
        true,
    )?;
    spawn(
        dispatcher,
        reservation,
        dispatch::request(Method::GET, uri, parts.headers, context, Body::empty()),
    );
    Ok(ack())
}

fn metadata(headers: &HeaderMap, name: &str) -> Result<String, Failure> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("Missing {name}")))
}

pub(crate) async fn stream_mux_request(
    Extension(dispatcher): Extension<Dispatcher>,
    Path((session, id)): Path<(String, String)>,
    request: Request,
) -> Result<Ack, Failure> {
    let context = Context::from_request(&request);
    let (parts, body) = request.into_parts();
    let path = metadata(&parts.headers, "x-domios-mux-path")?;
    let uri = model::validate_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let method: Method = metadata(&parts.headers, "x-domios-mux-method")?
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid mux method".into()))?;
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
    ) {
        return Err((StatusCode::BAD_REQUEST, "Unsupported mux method".into()));
    }
    let traffic = match metadata(&parts.headers, "x-domios-mux-class")?.as_str() {
        "interactive" => Traffic::Interactive,
        "bulk" => Traffic::Bulk,
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid mux class".into())),
    };
    let reservation = super::reserve(&session, &id, &parts.headers, traffic, false)?;
    let limit = if method == Method::POST && uri.path() == "/api/attachments/upload" {
        spool::UPLOAD_LIMIT
    } else {
        spool::ENVELOPE_LIMIT
    };
    if parts
        .headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|bytes| bytes > limit)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "Envelope exceeds its body limit".into(),
        ));
    }
    let stored = spool::receive(body, limit).await?;
    let relay = if uri.path() == "/api/mesh/relay-to" {
        stored.relay_policy().await
    } else {
        None
    };
    let requested_cancel = parts
        .headers
        .get("x-domios-mux-cancellable")
        .is_some_and(|v| v == "1");
    let cancellable = requested_cancel && model::cancellable(&method, uri.path(), relay.as_ref());
    reservation
        .entry
        .cancellable
        .store(cancellable, Ordering::Release);
    if reservation.entry.detached.load(Ordering::Acquire) && cancellable {
        return Ok(ack());
    }
    spawn(
        dispatcher,
        reservation,
        dispatch::request(method, uri, parts.headers, context, stored.into_body()),
    );
    Ok(ack())
}

fn spawn(dispatcher: Dispatcher, mut reservation: Reservation, request: Request) {
    let entry = reservation.entry.clone();
    let task = tokio::spawn(async move {
        let response = dispatcher.call(request).await;
        reservation.keep_queue = pump(&reservation, response).await
            || reservation.entry.terminal_queued.load(Ordering::Acquire);
        drop(reservation);
    });
    *entry.task.lock() = Some(task.abort_handle());
    if entry.detached.load(Ordering::Acquire) && entry.cancellable.load(Ordering::Acquire) {
        task.abort();
    }
}

async fn send(reservation: &Reservation, bytes: axum::body::Bytes, kind: Kind) -> bool {
    if reservation.entry.detached.load(Ordering::Acquire) {
        return false;
    }
    match reservation
        .scheduler
        .send(&reservation.id, bytes, kind)
        .await
    {
        Ok(()) => {
            if reservation.entry.stream {
                tokio::task::yield_now().await;
            }
            true
        }
        Err(Push::Overflow | Push::Oversized) => {
            reservation
                .entry
                .terminal_queued
                .store(true, Ordering::Release);
            reservation.scheduler.overflow(
                &reservation.id,
                frames::encode_end(
                    &reservation.id,
                    &ChannelEnd::Capacity {
                        bytes: super::scheduler::model::BYTE_BUDGET,
                    },
                ),
            );
            false
        }
        Err(_) => false,
    }
}

async fn pump(reservation: &Reservation, response: Response) -> bool {
    if !send(
        reservation,
        frames::encode_head(&reservation.id, response.status(), response.headers()),
        Kind::Head,
    )
    .await
    {
        return false;
    }
    let text = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.starts_with("text/")
                || v.contains("json")
                || v.contains("javascript")
                || v.contains("xml")
        });
    let mut upstream = response.into_body().into_data_stream();
    let mut tail = Vec::new();
    let cap = if text { 8 * 1024 } else { spool::IO_CHUNK };
    while let Some(chunk) = upstream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let message: String = error.to_string().chars().take(1024).collect();
                return send(
                    reservation,
                    frames::encode_end(&reservation.id, &ChannelEnd::Error { message }),
                    Kind::End,
                )
                .await;
            }
        };
        for part in chunk.chunks(cap) {
            tail.extend_from_slice(part);
            let size = if text {
                match std::str::from_utf8(&tail) {
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    _ => tail.len(),
                }
            } else {
                tail.len()
            };
            if size == 0 {
                continue;
            }
            let frame = frames::encode_chunk(&reservation.id, &tail[..size], text);
            if !send(reservation, frame, Kind::Data).await {
                return false;
            }
            tail.drain(..size);
        }
    }
    if !tail.is_empty()
        && !send(
            reservation,
            frames::encode_chunk(&reservation.id, &tail, false),
            Kind::Data,
        )
        .await
    {
        return false;
    }
    send(
        reservation,
        frames::encode_end(&reservation.id, &ChannelEnd::Eof),
        Kind::End,
    )
    .await
}
