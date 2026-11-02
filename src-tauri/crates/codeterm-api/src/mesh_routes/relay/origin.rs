use axum::http::{HeaderMap, StatusCode};

use crate::{auth::AuthenticatedOrigin, routes::AppState};

pub(super) fn peer_client(
    state: &AppState,
    headers: &HeaderMap,
    client: codeterm_service_core::mesh::transport::PeerClient,
) -> Result<codeterm_service_core::mesh::transport::PeerClient, (StatusCode, String)> {
    let origin = crate::auth::authenticated_origin(state, headers)?;
    let agent = crate::auth::authenticated_agent(state, headers)?;
    Ok(client.with_signed_token(sign(state, origin, agent.as_ref())?))
}

pub(super) fn loopback_request(
    state: &AppState,
    origin: AuthenticatedOrigin,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    body: &serde_json::Value,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
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
    let mut request = super::local_forward_http_client().request(method.clone(), url);
    if matches!(
        method,
        reqwest::Method::POST | reqwest::Method::PUT | reqwest::Method::PATCH
    ) {
        request = request.json(body);
    }
    request
        .bearer_auth(sign(state, origin, agent)?)
        .timeout(super::local_forward_deadline(&cmd.method, &cmd.path))
        .build()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}

pub(super) fn sign(
    state: &AppState,
    origin: AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
) -> Result<String, (StatusCode, String)> {
    let mesh = state.mesh.lock();
    let identity = &mesh.ident.identity;
    let key = identity.private_key.as_deref().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Mesh signing key unavailable".into(),
    ))?;
    codeterm_service_core::mesh::crown::sign_request_with_agent(
        key,
        &identity.instance_id,
        origin,
        agent,
    )
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
