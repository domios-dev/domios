use crate::routes::AppState;
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{header, StatusCode},
    response::Response,
};

use super::{
    copy_content_type, origin, peer_not_paired_error, peer_unroutable_error, validate_relay_path,
};

pub(super) fn is_relay_asset_path(rel: &str) -> bool {
    !rel.contains("..") && rel.starts_with("plugins/") && rel.contains("/ui/")
}

fn local_token_valid(state: &AppState, candidate: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    match state.config.lock().api.token {
        Some(ref t) if !t.is_empty() => codeterm_service_core::service::tokens::constant_time_eq(
            t.as_bytes(),
            candidate.as_bytes(),
        ),
        _ => false,
    }
}

#[derive(serde::Deserialize)]
pub struct RelayAssetQuery {
    token: Option<String>,
}

pub async fn mesh_relay_asset(
    AxumState(state): AxumState<AppState>,
    AxumPath((peer, rel)): AxumPath<(String, String)>,
    Query(q): Query<RelayAssetQuery>,
) -> Result<Response, (StatusCode, String)> {
    if !is_relay_asset_path(&rel) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("relay-asset limited to plugins/*/ui/*: {rel}"),
        ));
    }
    let needs_local_token = rel.ends_with(".html");
    let token_candidate = q.token.unwrap_or_default();
    let asset_state = state.clone();
    let (client, signed) = tokio::task::spawn_blocking(move || {
        if needs_local_token && !local_token_valid(&asset_state, &token_candidate) {
            return Err((StatusCode::UNAUTHORIZED, "bad or missing token".into()));
        }
        let ((primary, alternatives), (instance_id, private_key)) = {
            let mesh = asset_state.mesh.lock();
            let paired = mesh
                .paired_peers
                .get(&peer)
                .ok_or_else(|| peer_not_paired_error(&peer))?;
            if !codeterm_service_core::mesh::types::is_supported_relay_protocol(
                paired.relay_protocol_version,
            ) {
                return Err((
                    StatusCode::UPGRADE_REQUIRED,
                    origin::protocol_refusal(&paired.instance_name, paired.relay_protocol_version)
                        .1,
                ));
            }
            let endpoints = mesh
                .peer_route_endpoints(&peer)
                .ok_or_else(|| peer_unroutable_error(&paired.instance_name))?;
            let credentials = mesh.signing_credentials().ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Sign failed: No private key in identity".into(),
                )
            })?;
            (endpoints, credentials)
        };
        let header_token =
            codeterm_service_core::mesh::crown::sign_request(&private_key, &instance_id).map_err(
                |e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Sign failed: {e}"),
                    )
                },
            )?;
        let query_token =
            codeterm_service_core::mesh::crown::sign_request(&private_key, &instance_id).map_err(
                |e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Sign failed: {e}"),
                    )
                },
            )?;
        Ok((
            codeterm_service_core::mesh::transport::PeerClient::new_signed_route(
                primary,
                alternatives,
                &header_token,
            ),
            query_token,
        ))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    // token authenticates the read-only hop (see web_routes::validate_manifest_token).
    let path = format!("/api/{rel}?token={signed}");
    validate_relay_path(&path).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let resp = client.open_relay_stream(&path).await.map_err(|e| {
        (
            StatusCode::GATEWAY_TIMEOUT,
            format!("peer-unreachable: {e}"),
        )
    })?;
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = copy_content_type(&resp, "application/octet-stream");
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
    *out.status_mut() = status;
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    Ok(out)
}
