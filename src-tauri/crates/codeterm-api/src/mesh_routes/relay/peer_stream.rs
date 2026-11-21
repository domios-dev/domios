use crate::{
    routes::AppState,
    stream_census::{track, StreamGuard, StreamKind},
};
use axum::{
    body::Body,
    extract::{Query, State as AxumState},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};

use super::{
    copy_content_type, origin, peer_not_paired_error, peer_unroutable_error, validate_relay_path,
};

/// The only two cursor'd SSE streams a relay target may serve. Both carry D1
/// frames unmodified, including surface state, frames and notices.
pub(super) fn is_streamable_relay_path(bare: &str) -> bool {
    matches!(bare, "/api/events/stream" | "/api/output/stream")
        || surface_stream_tab(bare).is_some()
}

fn surface_stream_tab(path: &str) -> Option<&str> {
    let tab = path
        .strip_prefix("/api/surfaces/")?
        .strip_suffix("/stream")?;
    (!tab.is_empty() && !tab.contains('/') && !tab.contains('\\') && tab != "." && tab != "..")
        .then_some(tab)
}

pub(super) fn stream_unreachable(error: String) -> (StatusCode, String) {
    (
        StatusCode::GATEWAY_TIMEOUT,
        format!("peer-unreachable: {error}"),
    )
}

pub(super) async fn stream_refusal(response: reqwest::Response) -> (StatusCode, String) {
    let status = response.status();
    let message = response
        .text()
        .await
        .unwrap_or_else(|error| format!("Stream refusal could not be read: {error}"));
    (status, message)
}

#[derive(serde::Deserialize)]
pub struct RelayStreamQuery {
    pub peer: String,
    pub path: String,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub tab: Option<String>,
    scope: Option<crate::web_routes::PeerEventScope>,
    link: Option<String>,
}

/// `GET /api/mesh/relay-stream?peer=<id>&path=<streamable>&since=<cursor>` —
/// browser→local (local token). Opens a signed, un-timed stream to the peer's
/// own `/api/events|output/stream?since=` and pipes bytes verbatim. Peer
/// unreachable → 504 `peer-unreachable` so the client shows a banner + backoff,
/// never a silent poll.
pub async fn mesh_relay_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<RelayStreamQuery>,
) -> Result<Response, (StatusCode, String)> {
    let diagnostic_level = crate::stream_diagnostics::configured_level(
        state.config.lock().diagnostics.terminal_transport,
    );

    let bare = q.path.split('?').next().unwrap_or(&q.path);
    if !is_streamable_relay_path(bare) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Not a streamable relay path: {bare}"),
        ));
    }
    // so a raw append needs no percent-encoding.
    let mut path = q.path.clone();
    let mut stream_tab_id = None;
    let shared = matches!(q.scope, Some(crate::web_routes::PeerEventScope::Peer));
    if q.scope.is_some() && !shared {
        return Err((
            StatusCode::BAD_REQUEST,
            "Unsupported relay stream scope".into(),
        ));
    }
    if shared {
        if !matches!(
            crate::auth::caller_identity(&state, &headers)?,
            crate::auth::CallerIdentity::Local
        ) {
            return Err((
                StatusCode::FORBIDDEN,
                "Peer rail requires a local owner".into(),
            ));
        }
        if q.path != "/api/events/stream" || q.tab.is_some() {
            return Err((
                StatusCode::BAD_REQUEST,
                "Peer scope requires an unfiltered event stream".into(),
            ));
        }
        path = format!("{bare}?scope=peer");
    } else if let Some(ordinary_id) = q.tab.as_deref() {
        let owner =
            super::super::remote_tabs::validate_remote_stream_owner(&q.peer, ordinary_id, bare)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        stream_tab_id = Some(owner.tab_id.clone());
        let mut params = url::form_urlencoded::Serializer::new(String::new());
        params.append_pair("tab_id", &owner.tab_id);
        if bare == "/api/events/stream" {
            params.append_pair("scope", "tab");
        }
        path = format!("{bare}?{}", params.finish());
    } else if bare == "/api/events/stream" {
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}scope=notifications");
    } else if bare == "/api/output/stream" {
        return Err((
            StatusCode::BAD_REQUEST,
            "remote tab stream requires composite tab".into(),
        ));
    }
    if let Some(since) = q.since.as_deref().filter(|s| !s.is_empty()) {
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}since={since}");
    }
    validate_relay_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let client = {
        let mesh = state.mesh.lock();
        let peer = mesh
            .paired_peers
            .get(&q.peer)
            .ok_or_else(|| peer_not_paired_error(&q.peer))?;
        if shared {
            let expected = q
                .link
                .as_deref()
                .and_then(|link| serde_json::from_str::<[String; 2]>(link).ok());
            if !expected.is_some_and(|[key, paired_at]| {
                key == peer.public_key && paired_at == peer.paired_at
            }) {
                return Err((StatusCode::CONFLICT, "Peer rail pairing changed".into()));
            }
        }
        if !peer.online {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "remote tab unavailable".into(),
            ));
        }
        if !codeterm_service_core::mesh::types::is_supported_relay_protocol(
            peer.relay_protocol_version,
        ) {
            return Err((
                StatusCode::UPGRADE_REQUIRED,
                origin::protocol_refusal(&peer.instance_name, peer.relay_protocol_version).1,
            ));
        }
        mesh.client_for(&q.peer)
            .ok_or_else(|| peer_unroutable_error(&peer.instance_name))?
    };
    let client = origin::peer_client(&state, &headers, client)?;
    if shared {
        return super::peer_rail::subscribe(client, state.mesh.clone(), q.peer, path).await;
    }
    let resp = client
        .open_relay_stream(&path)
        .await
        .map_err(stream_unreachable)?;
    if !resp.status().is_success() {
        return Err(stream_refusal(resp).await);
    }
    let content_type = copy_content_type(&resp, "text/event-stream");
    let body = if bare == "/api/output/stream" {
        super::super::relay_stream::output_body(
            resp.bytes_stream(),
            diagnostic_level,
            q.tab.clone().unwrap_or_else(|| "*".into()),
            stream_tab_id,
            q.since.as_deref(),
            StreamGuard::relayed(StreamKind::Output, q.peer.clone()),
        )
    } else {
        Body::from_stream(track(
            resp.bytes_stream(),
            StreamGuard::relayed(
                if surface_stream_tab(bare).is_some() {
                    StreamKind::Surface
                } else {
                    StreamKind::Events
                },
                q.peer.clone(),
            ),
        ))
    };
    let body = super::super::account_link::fence_body(body, state.mesh.clone(), q.peer.clone());
    let mut out = Response::new(body);
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    out.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(out)
}
