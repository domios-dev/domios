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
    let (origin, agent) = authenticated_local_caller(state, headers)?;
    let token = sign(state, origin, agent.as_ref())?;
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

#[cfg(test)]
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

pub(super) enum LocalForwardRequest {
    InProcess(axum::Router, axum::http::Request<axum::body::Body>),
    TcpLoopback(reqwest::Request),
}

pub(super) enum LocalForwardOutcome {
    Response {
        status: u16,
        is_json: bool,
        body: Result<axum::body::Bytes, String>,
    },
    Failed(codeterm_service_core::mesh::types::RelayResponse),
}

pub(super) fn build_local_forward(
    state: &AppState,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    body: &serde_json::Value,
    authorization: axum::http::HeaderValue,
    sender_name: &str,
) -> Result<LocalForwardRequest, (StatusCode, String)> {
    if let Some(router) = state
        .svc
        .try_ext::<codeterm_service_core::ports::rest::LocalApiRouter>()
        .map(|router| router.0.clone())
    {
        log::debug!(
            "[mesh/relay] branch=in_process method={} path={} source={sender_name}",
            cmd.method,
            cmd.path
        );
        Ok(LocalForwardRequest::InProcess(
            router,
            in_process_request_with_auth(cmd, body, authorization)?,
        ))
    } else {
        log::debug!(
            "[mesh/relay] branch=loopback method={} path={} source={sender_name}",
            cmd.method,
            cmd.path
        );
        Ok(LocalForwardRequest::TcpLoopback(
            loopback_request_with_auth(state, cmd, body, authorization)?,
        ))
    }
}

pub(super) async fn execute_local_forward(
    target: LocalForwardRequest,
    deadline: std::time::Duration,
) -> Result<LocalForwardOutcome, (StatusCode, String)> {
    match target {
        LocalForwardRequest::InProcess(router, request) => {
            use tower::ServiceExt as _;
            const MAX_LOCAL_FORWARD_BODY_BYTES: usize = 64 * 1024 * 1024;
            let forwarded = tokio::time::timeout(deadline, async move {
                let resp = router
                    .oneshot(request)
                    .await
                    .unwrap_or_else(|never| match never {});
                let status = resp.status().as_u16();
                let is_json = resp
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.split(';').next())
                    .is_some_and(|value| value.eq_ignore_ascii_case("application/json"));
                let body = axum::body::to_bytes(resp.into_body(), MAX_LOCAL_FORWARD_BODY_BYTES)
                    .await
                    .map_err(|error| error.to_string());
                (status, is_json, body)
            })
            .await;
            match forwarded {
                Ok((status, is_json, body)) => Ok(LocalForwardOutcome::Response {
                    status,
                    is_json,
                    body,
                }),
                Err(_) => Ok(LocalForwardOutcome::Failed(
                    super::forward_failure_response(
                        false,
                        true,
                        "in-process local relay timed out".into(),
                    ),
                )),
            }
        }
        LocalForwardRequest::TcpLoopback(request) => {
            let client = super::local_forward_http_client()?;
            let resp = match client.execute(request).await {
                Ok(resp) => resp,
                Err(error) => {
                    return Ok(LocalForwardOutcome::Failed(super::local_forward_failure(
                        &error,
                    )))
                }
            };
            let status = resp.status().as_u16();
            let is_json = resp
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .is_some_and(|value| value.eq_ignore_ascii_case("application/json"));
            let body = resp.bytes().await.map_err(|error| error.to_string());
            Ok(LocalForwardOutcome::Response {
                status,
                is_json,
                body,
            })
        }
    }
}

pub(super) fn in_process_request_with_auth(
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    body: &serde_json::Value,
    authorization: axum::http::HeaderValue,
) -> Result<axum::http::Request<axum::body::Body>, (StatusCode, String)> {
    let method = cmd.method.to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unsupported method: {}", cmd.method),
        ));
    }
    let method = axum::http::Method::from_bytes(method.as_bytes())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let mut builder = axum::http::Request::builder()
        .method(method.clone())
        .uri(&cmd.path)
        .header(axum::http::header::AUTHORIZATION, authorization);
    let body = if matches!(
        method,
        axum::http::Method::POST | axum::http::Method::PUT | axum::http::Method::PATCH
    ) {
        let bytes = serde_json::to_vec(body)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        builder = builder.header(axum::http::header::CONTENT_TYPE, "application/json");
        axum::body::Body::from(bytes)
    } else {
        axum::body::Body::empty()
    };
    builder
        .body(body)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
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

pub(crate) fn sign(
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
