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
use codeterm_neighbors_service::prelude::NeighborsAccountMesh1Ext as _;

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
        let caller =
            crate::auth::off_runtime(&state, &headers, crate::auth::caller_identity).await?;
        if !matches!(caller, crate::auth::CallerIdentity::Local) {
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

    let (peer_id, link) = (q.peer.clone(), q.link.clone());
    let (diagnostic_level, client, link_nonce) =
        crate::auth::off_runtime(&state, &headers, move |state, headers| {
            let diagnostic_level = crate::stream_diagnostics::configured_level(
                state.config.lock().diagnostics.terminal_transport,
            );
            let ((primary, alternatives), link_nonce) = {
                let mesh = state.mesh.lock();
                let peer = mesh
                    .paired_peers
                    .get(&peer_id)
                    .ok_or_else(|| peer_not_paired_error(&peer_id))?;
                if shared {
                    let expected = link
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
                if !mesh.peer_requests_allowed(&peer_id) {
                    return Err((
                        StatusCode::SERVICE_UNAVAILABLE,
                        "peer-reconnecting: authentication backoff active".into(),
                    ));
                }
                if !codeterm_service_core::mesh::types::is_supported_relay_protocol(
                    peer.relay_protocol_version,
                ) {
                    return Err((
                        StatusCode::UPGRADE_REQUIRED,
                        origin::protocol_refusal(&peer.instance_name, peer.relay_protocol_version)
                            .1,
                    ));
                }
                let link_nonce = mesh
                    .link_record(&peer_id)
                    .and_then(|record| record.link_nonce.clone());
                let endpoints = mesh
                    .peer_route_endpoints(&peer_id)
                    .ok_or_else(|| peer_unroutable_error(&peer.instance_name))?;
                (endpoints, link_nonce)
            };
            let (_, client) = origin::signed_peer_client(state, headers, primary, alternatives)?;
            Ok((diagnostic_level, client, link_nonce))
        })
        .await?;
    if let Err(remaining_ms) =
        super::peer_stream_backoff::check_allowed(&state.mesh, &q.peer, link_nonce.as_deref())
    {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("peer-reconnecting: retry after {remaining_ms}ms"),
        ));
    }
    if shared {
        return super::peer_rail::subscribe(
            client,
            state.mesh.clone(),
            state.svc.clone(),
            q.peer,
            link_nonce,
            path,
        )
        .await;
    }
    let resp = client
        .open_relay_stream(&path)
        .await
        .map_err(stream_unreachable)?;
    if !resp.status().is_success() {
        if resp.status() == StatusCode::UNAUTHORIZED {
            let outcome = super::peer_stream_backoff::record_refusal(
                &state.mesh,
                &q.peer,
                link_nonce.as_deref(),
            );
            if outcome.trigger_refresh {
                state.svc.spawn_account_mesh_refresh(
                    codeterm_account::account_mesh::EnrollTrigger::Lease,
                );
            }
        }
        return Err(stream_refusal(resp).await);
    }
    super::peer_stream_backoff::record_success(&state.mesh, &q.peer);
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
