use axum::http::{HeaderMap, StatusCode};
use codeterm_mesh::{
    crown::{AppearanceStoreScope, AuthenticatedAgent},
    types::RelayCommand,
};

use crate::{
    appearance_routes::authority,
    auth::{peer_grant::PeerGrant, AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

#[cfg(test)]
pub(super) fn peer_client(
    state: &AppState,
    headers: &HeaderMap,
    client: codeterm_service_core::mesh::transport::PeerClient,
) -> Result<codeterm_service_core::mesh::transport::PeerClient, (StatusCode, String)> {
    let (_, token) = signed_token_for_local_caller(state, headers, None)?;
    Ok(client.with_signed_token(token))
}

pub(super) fn signed_peer_client(
    state: &AppState,
    headers: &HeaderMap,
    primary: String,
    alternatives: Vec<String>,
    store: Option<AppearanceStoreScope>,
) -> Result<
    (
        AuthenticatedOrigin,
        codeterm_service_core::mesh::transport::PeerClient,
    ),
    (StatusCode, String),
> {
    let (origin, token) = signed_token_for_local_caller(state, headers, store)?;
    Ok((
        origin,
        codeterm_service_core::mesh::transport::PeerClient::new_signed_route(
            primary,
            alternatives,
            &token,
        ),
    ))
}

pub(super) fn signed_token_for_local_caller(
    state: &AppState,
    headers: &HeaderMap,
    store: Option<AppearanceStoreScope>,
) -> Result<(AuthenticatedOrigin, String), (StatusCode, String)> {
    let (caller, origin) = crate::auth::authenticated_request(state, headers)?;
    if !matches!(caller, CallerIdentity::Local | CallerIdentity::Tab(_)) {
        return Err((
            StatusCode::FORBIDDEN,
            "Outbound mesh signing requires a local caller".into(),
        ));
    }
    let agent = crate::auth::authenticated_agent_for_origin(state, headers, origin)?;
    if let (CallerIdentity::Tab(caller), Some(scope)) = (&caller, store) {
        let agent = agent
            .as_ref()
            .filter(|_| crate::auth::is_bound_main(state, caller))
            .ok_or((
                StatusCode::FORBIDDEN,
                authority::MAIN_OR_OWNER_REQUIRED.into(),
            ))?;
        let (_, key) = signing_credentials(state)?;
        let token =
            codeterm_mesh::crown::sign_appearance_store(&key, agent, scope).map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Sign failed: {error}"),
                )
            })?;
        return Ok((origin, token));
    }
    Ok((origin, sign(state, origin, agent.as_ref())?))
}

pub(crate) struct AdmittedAppearanceStore {
    grant: PeerGrant,
    machine: String,
    body: serde_json::Value,
}

impl AdmittedAppearanceStore {
    pub(crate) fn into_parts(self) -> (PeerGrant, String, serde_json::Value) {
        (self.grant, self.machine, self.body)
    }
}

type OrdinaryIdentity = (String, AuthenticatedOrigin, Option<AuthenticatedAgent>);

pub(super) enum RelayAdmission {
    Ordinary(OrdinaryIdentity),
    AppearanceStore(AdmittedAppearanceStore),
}

/// Single-pass Ed25519 verification + nonce admission for inbound signed mesh requests.
pub(crate) fn authenticate_signed_mesh_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<OrdinaryIdentity, (StatusCode, String)> {
    match authenticate_relay(state, headers, None)? {
        RelayAdmission::Ordinary(identity) => Ok(identity),
        RelayAdmission::AppearanceStore(_) => Err(invalid_store_scope()),
    }
}

pub(super) fn authenticate_relay(
    state: &AppState,
    headers: &HeaderMap,
    command: Option<&RelayCommand>,
) -> Result<RelayAdmission, (StatusCode, String)> {
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
    let bound_store = claims
        .appearance_store
        .as_ref()
        .map(|scope| bound_store_command(scope, command, &our_id))
        .transpose()?;
    if claims.from_instance_id == our_id {
        if bound_store.is_some() {
            return Err(invalid_store_scope());
        }
        let agent = (claims.origin == AuthenticatedOrigin::Agent)
            .then_some(claims.agent)
            .flatten();
        return Ok(RelayAdmission::Ordinary((
            "local".to_string(),
            claims.origin,
            agent,
        )));
    }
    let peer_id = claims.from_instance_id;
    {
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
        let grant = PeerGrant::capture(&mesh, &peer_id).ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or unknown signature".to_string(),
            )
        })?;
        if bound_store.is_some() {
            if claims.origin != AuthenticatedOrigin::Agent {
                return Err(invalid_store_scope());
            }
            if !codeterm_mesh::types::is_supported_relay_protocol(peer.relay_protocol_version) {
                return Err(protocol_refusal(
                    &peer.instance_name,
                    peer.relay_protocol_version,
                ));
            }
            authority::authorize_admitted_store(grant.access())?;
        }
        mesh.check_and_record_nonce_id(claims.nonce)
            .map_err(|error| (StatusCode::UNAUTHORIZED, error))?;
        if let Some((machine, body)) = bound_store {
            drop(mesh);
            return Ok(RelayAdmission::AppearanceStore(AdmittedAppearanceStore {
                grant,
                machine,
                body: body.clone(),
            }));
        }
    }
    let agent = (claims.origin == AuthenticatedOrigin::Agent)
        .then_some(claims.agent)
        .flatten();
    Ok(RelayAdmission::Ordinary((peer_id, claims.origin, agent)))
}

fn bound_store_command<'a>(
    scope: &AppearanceStoreScope,
    command: Option<&'a RelayCommand>,
    destination: &str,
) -> Result<(String, &'a serde_json::Value), (StatusCode, String)> {
    let command = command.ok_or_else(invalid_store_scope)?;
    let machine = authority::proposal_machine(&command.method, &command.path)
        .ok_or_else(invalid_store_scope)?;
    super::validate_relay_path(&command.path)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let expected =
        AppearanceStoreScope::new(destination, &command.method, &command.path, &command.body)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    if *scope != expected {
        return Err(invalid_store_scope());
    }
    let segment = format!(
        "machine={}",
        machine.replace('+', "%2B").replace('&', "%26")
    );
    let (_, machine) = url::form_urlencoded::parse(segment.as_bytes())
        .next()
        .ok_or_else(invalid_store_scope)?;
    Ok((machine.into_owned(), &command.body))
}

fn invalid_store_scope() -> (StatusCode, String) {
    (
        StatusCode::FORBIDDEN,
        "Signed appearance proposal does not match this relay request.".into(),
    )
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
    let (instance_id, key) = signing_credentials(state)?;
    codeterm_service_core::mesh::crown::sign_request_with_agent(&key, &instance_id, origin, agent)
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sign failed: {error}"),
            )
        })
}

fn signing_credentials(state: &AppState) -> Result<(String, String), (StatusCode, String)> {
    state.mesh.lock().signing_credentials().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Mesh signing key unavailable".into(),
    ))
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
