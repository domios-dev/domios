use axum::http::{HeaderMap, StatusCode};

use crate::{
    auth::{peer_grant::PeerGrant, AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

#[derive(Debug)]
pub(crate) struct SignedMeshRequest {
    pub peer_id: String,
    pub origin: AuthenticatedOrigin,
    pub agent: Option<codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    pub grant: Option<PeerGrant>,
}

#[cfg(test)]
pub(super) fn peer_client(
    state: &AppState,
    headers: &HeaderMap,
    client: codeterm_service_core::mesh::transport::PeerClient,
) -> Result<codeterm_service_core::mesh::transport::PeerClient, (StatusCode, String)> {
    let (_, token) = signed_token_for_local_caller(state, headers)?;
    Ok(client.with_signed_token(token))
}

pub(super) fn signed_peer_client(
    state: &AppState,
    headers: &HeaderMap,
    primary: String,
    alternatives: Vec<String>,
) -> Result<
    (
        AuthenticatedOrigin,
        codeterm_service_core::mesh::transport::PeerClient,
    ),
    (StatusCode, String),
> {
    let (origin, token) = signed_token_for_local_caller(state, headers)?;
    Ok((
        origin,
        codeterm_service_core::mesh::transport::PeerClient::new_signed_route(
            primary,
            alternatives,
            &token,
        ),
    ))
}

pub(super) fn authenticated_local_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<
    (
        AuthenticatedOrigin,
        Option<codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    ),
    (StatusCode, String),
> {
    let (caller, origin) = crate::auth::authenticated_request(state, headers)?;
    if !matches!(caller, CallerIdentity::Local | CallerIdentity::Tab(_)) {
        return Err((
            StatusCode::FORBIDDEN,
            "Outbound mesh signing requires a local caller".into(),
        ));
    }
    let agent = crate::auth::authenticated_agent_for_origin(state, headers, origin)?;
    Ok((origin, agent))
}

pub(super) fn signed_caller_token(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<
    (
        AuthenticatedOrigin,
        Option<codeterm_service_core::mesh::crown::AuthenticatedAgent>,
        String,
    ),
    (StatusCode, String),
> {
    let (origin, agent) = authenticated_local_caller(state, headers)?;
    let token = sign(state, origin, agent.as_ref())?;
    Ok((origin, agent, token))
}

fn signed_token_for_local_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(AuthenticatedOrigin, String), (StatusCode, String)> {
    let (origin, _, token) = signed_caller_token(state, headers)?;
    Ok((origin, token))
}

/// Single-pass Ed25519 verification + nonce admission for inbound signed mesh requests.
pub(crate) fn authenticate_signed_mesh_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<SignedMeshRequest, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let (known_keys, our_id) = crate::auth::mesh_keys(state);
    let claims = codeterm_service_core::mesh::crown::verify_request_claims(token, &known_keys)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            )
        })?;
    if claims.from_instance_id == our_id {
        let agent = (claims.origin == AuthenticatedOrigin::Agent)
            .then_some(claims.agent)
            .flatten();
        return Ok(SignedMeshRequest {
            peer_id: "local".into(),
            origin: claims.origin,
            agent,
            grant: None,
        });
    }
    let peer_id = claims.from_instance_id;
    let grant = {
        let mut mesh = state.mesh.lock();
        let Some(peer) = mesh.paired_peers.get(&peer_id) else {
            return Err((
                StatusCode::UNAUTHORIZED,
                format!("Peer {} not paired", peer_id),
            ));
        };
        if known_keys.get(&peer_id) != Some(&peer.public_key) {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            ));
        }
        if claims.origin == AuthenticatedOrigin::Agent
            && claims
                .agent
                .as_ref()
                .is_none_or(|agent| agent.instance_id != peer_id)
        {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            ));
        }
        let Some(grant) = PeerGrant::capture(&mesh, &peer_id) else {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            ));
        };
        mesh.check_and_record_nonce_id(claims.nonce)
            .map_err(|error| (StatusCode::UNAUTHORIZED, error))?;
        grant
    };
    let agent = (claims.origin == AuthenticatedOrigin::Agent)
        .then_some(claims.agent)
        .flatten();
    Ok(SignedMeshRequest {
        peer_id,
        origin: claims.origin,
        agent,
        grant: Some(grant),
    })
}

pub(super) fn loopback_request(
    state: &AppState,
    origin: AuthenticatedOrigin,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    body: &serde_json::Value,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
) -> Result<reqwest::Request, (StatusCode, String)> {
    let token = sign(state, origin, agent)?;
    loopback_request_with_auth(
        state,
        cmd,
        body,
        format!("Bearer {token}").parse().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid signed authorization".into(),
            )
        })?,
    )
}

pub(super) fn loopback_request_with_auth(
    state: &AppState,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    body: &serde_json::Value,
    authorization: axum::http::HeaderValue,
) -> Result<reqwest::Request, (StatusCode, String)> {
    let method = cmd.method.to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unsupported method: {}", cmd.method),
        ));
    }
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
    let mut request = super::local_forward_http_client()?.request(method.clone(), url);
    if matches!(
        method,
        reqwest::Method::POST | reqwest::Method::PUT | reqwest::Method::PATCH
    ) {
        request = request.json(body);
    }
    request
        .header(axum::http::header::AUTHORIZATION, authorization)
        .timeout(super::local_forward_deadline(&cmd.method, &cmd.path))
        .build()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}

pub(super) fn sign(
    state: &AppState,
    origin: AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
) -> Result<String, (StatusCode, String)> {
    let (instance_id, key) = state.mesh.lock().signing_credentials().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Mesh signing key unavailable".into(),
    ))?;
    codeterm_service_core::mesh::crown::sign_request_with_agent(&key, &instance_id, origin, agent)
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sign failed: {error}"),
            )
        })
}

pub(super) fn protocol_refusal(peer_name: &str, protocol: u16) -> (StatusCode, String) {
    let message = format!(
        "Peer {peer_name} runs an older or incompatible Domios (Mesh relay protocol {protocol}); update it through the updater to preserve authenticated origin."
    );
    (
        StatusCode::UPGRADE_REQUIRED,
        serde_json::json!({
            "code": codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
            "reason": codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH,
            "message": message,
            "error": {"code": codeterm_mesh::contract::delivery::PEER_RELAY_PROTOCOL_MISMATCH, "message": message},
            "retry_hint": "Update the peer through the updater, then retry.",
            "bytes_written": false,
            "peer": peer_name,
            "relay_protocol_version": protocol,
            "required_relay_protocol_version": codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
        }).to_string(),
    )
}
