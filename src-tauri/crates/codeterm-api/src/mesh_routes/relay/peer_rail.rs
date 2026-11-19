use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, LazyLock, Weak},
};

use axum::{
    body::{Body, Bytes},
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use codeterm_service_core::{mesh::transport::PeerClient, MeshStateHandle};
use futures_util::StreamExt;
use parking_lot::Mutex;
use tokio::sync::{broadcast, watch};

use crate::{
    auth::peer_grant::PeerGrant,
    stream_census::{track, StreamGuard, StreamKind},
};

type Refusal = (StatusCode, String);
type RailKey = (usize, String, String);
type Rails = HashMap<RailKey, Weak<Rail>>;

const FRAME_LIMIT: usize = 64 * 1024;
const QUEUE_CAPACITY: usize = 64;
const SILENCE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

static RAILS: LazyLock<Mutex<Rails>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
enum Status {
    Opening,
    Open,
    Closed(Option<Refusal>),
}

struct Rail {
    sender: broadcast::Sender<Bytes>,
    boundary: Mutex<Option<Bytes>>,
    status: watch::Receiver<Status>,
}

pub(super) async fn subscribe(
    client: PeerClient,
    mesh: MeshStateHandle,
    peer_id: String,
    path: String,
) -> Result<Response, Refusal> {
    let (key, grant) = {
        let manager = mesh.lock();
        let link = manager
            .link_record(&peer_id)
            .and_then(|record| record.link_nonce.clone());
        let grant = PeerGrant::capture(&manager, &peer_id);
        let (Some(link), Some(grant)) = (link, grant) else {
            return Err((StatusCode::FORBIDDEN, "Peer link is unavailable".into()));
        };
        ((Arc::as_ptr(&mesh) as usize, peer_id.clone(), link), grant)
    };
    let (rail, boundary, mut receiver) = {
        let mut rails = RAILS.lock();
        rails.retain(|_, rail| rail.strong_count() > 0);
        if let Some(rail) = rails
            .get(&key)
            .and_then(Weak::upgrade)
            .filter(|rail| !matches!(&*rail.status.borrow(), Status::Closed(_)))
        {
            let (boundary, receiver) = {
                let boundary = rail.boundary.lock();
                (boundary.clone(), rail.sender.subscribe())
            };
            (rail, boundary, receiver)
        } else {
            let (sender, receiver) = broadcast::channel(QUEUE_CAPACITY);
            let (status_tx, status) = watch::channel(Status::Opening);
            let rail = Arc::new(Rail {
                sender,
                boundary: Mutex::new(None),
                status,
            });
            rails.insert(key, Arc::downgrade(&rail));
            tokio::spawn(drive_rail(
                client,
                mesh,
                grant,
                peer_id,
                path,
                rail.clone(),
                status_tx,
            ));
            (rail, None, receiver)
        }
    };
    let mut status = rail.status.clone();
    loop {
        let current = status.borrow_and_update().clone();
        match current {
            Status::Opening if status.changed().await.is_ok() => {}
            Status::Open => break,
            Status::Closed(Some(refusal)) => return Err(refusal),
            _ => {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "Peer rail closed before admission".into(),
                ))
            }
        }
    }

    let body = Body::from_stream(async_stream::stream! {
        if let Some(boundary) = boundary {
            if matches!(&*status.borrow(), Status::Open) {
                yield Ok::<_, Infallible>(boundary);
            }
        }
        loop {
            if !matches!(&*status.borrow_and_update(), Status::Open) {
                break;
            }
            let frame = tokio::select! {
                biased;
                _ = status.changed() => break,
                frame = receiver.recv() => frame,
            };
            let Ok(frame) = frame else { break };
            if !matches!(&*status.borrow(), Status::Open) {
                break;
            }
            yield Ok::<_, Infallible>(frame);
        }
    });
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

async fn drive_rail(
    client: PeerClient,
    mesh: MeshStateHandle,
    grant: PeerGrant,
    peer_id: String,
    path: String,
    rail: Arc<Rail>,
    status: watch::Sender<Status>,
) {
    let upstream = async {
        let response = tokio::time::timeout(SILENCE_DEADLINE, client.open_relay_stream(&path))
            .await
            .map_err(|_| {
                super::peer_stream::stream_unreachable("Peer rail admission timed out".into())
            })?
            .map_err(super::peer_stream::stream_unreachable)?;
        if !response.status().is_success() {
            return Err(super::peer_stream::stream_refusal(response).await);
        }
        status.send_replace(Status::Open);
        let mut stream = std::pin::pin!(track(
            response.bytes_stream(),
            StreamGuard::relayed(StreamKind::Events, peer_id.clone())
        ));
        let mut buffer = Vec::new();
        loop {
            let item = tokio::time::timeout(SILENCE_DEADLINE, stream.next())
                .await
                .map_err(|_| {
                    super::peer_stream::stream_unreachable("Peer rail went silent".into())
                })?;
            let Some(chunk) = item else { break };
            let chunk =
                chunk.map_err(|error| super::peer_stream::stream_unreachable(error.to_string()))?;
            for segment in chunk.split_inclusive(|byte| *byte == b'\n') {
                if buffer.len() + segment.len() > FRAME_LIMIT {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        "Peer rail frame exceeds its bound".into(),
                    ));
                }
                buffer.extend_from_slice(segment);
                if !buffer.ends_with(b"\n\n") {
                    continue;
                }
                let frame = Bytes::from(std::mem::take(&mut buffer));
                if !grant.current(&mesh.lock()) {
                    return Ok(());
                }
                let mut marker = rail.boundary.lock();
                if is_boundary(&frame) {
                    *marker = Some(frame.clone());
                }
                let _ = rail.sender.send(frame);
            }
        }
        Ok(())
    };
    let ended = tokio::select! {
        biased;
        _ = grant.changed(&mesh) => Ok(()),
        result = upstream => result,
    };
    if matches!(
        &ended,
        Err((StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED, _))
    ) {
        mesh.lock().peer_services.prune(&peer_id);
    } else {
        mesh.lock().peer_services.invalidate(&peer_id);
    }
    status.send_replace(Status::Closed(ended.err()));
}

fn is_boundary(frame: &[u8]) -> bool {
    frame
        .strip_prefix(b"data: ")
        .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
        .and_then(|value| {
            value
                .get("event_type")
                .and_then(serde_json::Value::as_str)
                .map(|event| event == "peer-replay-complete")
        })
        .unwrap_or(false)
}
