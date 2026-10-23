use axum::{
    extract::{ConnectInfo, Request, State as AxumState},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use super::routes::AppState;
use codeterm_service_core::api_support::access_policy::peer_access_allowed;
use codeterm_service_core::mesh::types::AccessLevel;
use codeterm_service_core::service::tokens::token_fp;

#[derive(Clone, Debug)]
pub enum CallerIdentity {
    Local,
    ServicePush {
        service_id: String,
    },
    Peer {
        instance_id: String,
        access_level: AccessLevel,
        relay_protocol_version: u16,
    },
}

pub fn is_service_push_allowed(method: &str, path: &str) -> bool {
    if method != "POST" {
        return false;
    }
    let Some(rest) = path.strip_prefix("/api/tabs/") else {
        return false;
    };
    let Some(tab_id) = rest.strip_suffix("/send") else {
        return false;
    };
    !tab_id.is_empty() && !tab_id.contains('/')
}

fn tab_hook_target<'a>(method: &Method, path: &'a str) -> Option<(&'a str, &'a str)> {
    if method != Method::POST {
        return None;
    }
    let rest = path.strip_prefix("/api/tabs/")?;
    let (tab_id, event) = rest.split_once("/hook/")?;
    if tab_id.is_empty() || tab_id.contains('/') {
        return None;
    }
    matches!(event, "session_start" | "provider_event" | "stop").then_some((tab_id, event))
}

/// The link handshake authenticates by signed membership, installation possession and
/// both online permits; nothing on these paths can create a pair without them.
pub(crate) const PUBLIC_PATHS: &[&str] = &[
    "/api/health",
    "/api/mesh/ping",
    "/api/mesh/link/hello",
    "/api/mesh/link/confirm",
    "/api/mesh/link/commit",
];

fn bearer_exempt(method: &Method, path: &str) -> bool {
    !path.starts_with("/api/") || PUBLIC_PATHS.contains(&path) || is_custom_auth(method, path)
}

fn peer_protocol_allowed(version: u16, _path: &str) -> bool {
    codeterm_service_core::mesh::types::is_supported_relay_protocol(version)
}

fn peer_protocol_refusal(version: u16) -> Option<String> {
    (!peer_protocol_allowed(version, ""))
        .then(codeterm_service_core::mesh::version::peer_upgrade_message)
}

fn announce_judges_own_protocol(method: &Method, path: &str) -> bool {
    method == Method::POST && path == "/api/mesh/announce"
}

// A signed announce is the only writer of the stored protocol, so it is judged by its own body.
fn stored_protocol_refusal(method: &Method, path: &str, stored_version: u16) -> Option<String> {
    if announce_judges_own_protocol(method, path) {
        return None;
    }
    peer_protocol_refusal(stored_version)
}

#[derive(Debug, PartialEq, Eq)]
enum StoredProtocolAnswer {
    Proceed,
    EmptyGossip,
    UpgradeRequired(String),
}

// A v1.14 peer retries a failed sync on every 15s health tick and only backs off to its
// periodic interval after a successful one, so an empty gossip snapshot is its sole backoff.
fn stored_protocol_answer(
    method: &Method,
    path: &str,
    stored_version: u16,
) -> StoredProtocolAnswer {
    match stored_protocol_refusal(method, path, stored_version) {
        None => StoredProtocolAnswer::Proceed,
        Some(_) if method == Method::POST && path == "/api/mesh/sync" => {
            StoredProtocolAnswer::EmptyGossip
        }
        Some(message) => StoredProtocolAnswer::UpgradeRequired(message),
    }
}

fn empty_gossip_snapshot() -> codeterm_service_core::mesh::types::MeshSyncPayload {
    codeterm_service_core::mesh::types::MeshSyncPayload {
        peers: Vec::new(),
        generation: 0,
    }
}

fn record_peer_protocol(state: &AppState, instance_id: &str, stored_version: u16) {
    use codeterm_service_core::mesh::protocol_refusals::ProtocolRefusalTransition;
    let refused = peer_protocol_refusal(stored_version).is_some();
    let transition = state
        .mesh
        .lock()
        .protocol_refusals
        .observe(instance_id, refused);
    match transition {
        Some(ProtocolRefusalTransition::Refused) => log::warn!(
            "[auth] Refusing peer {} on relay protocol {} until it upgrades; later requests are answered silently",
            instance_id,
            stored_version
        ),
        Some(ProtocolRefusalTransition::Accepted) => log::info!(
            "[auth] Peer {} now uses relay protocol {}; refusal cleared",
            instance_id,
            stored_version
        ),
        None => {}
    }
}

fn requires_mesh_network(method: &Method, path: &str) -> bool {
    path.starts_with("/api/mesh/")
        && path != "/api/mesh/ping"
        && path != "/api/mesh/enabled"
        && !(method == Method::GET && path == "/api/mesh/inventory")
}

fn remote_network_allowed(mesh_enabled: bool, address: Option<SocketAddr>) -> bool {
    mesh_enabled || address.is_none_or(|address| address.ip().is_loopback())
}

/// Paths whose handlers authenticate independently and must skip bearer middleware.
/// This is an EXACT allowlist on purpose: an earlier `starts_with("/api/mesh/peers/")`
/// prefix match exempted every peer sub-route whose handlers only call the no-op
/// `check_auth`, leaving them unauthenticated. Only `relay`, the adoption pair and
/// the unpair handler (`DELETE /api/mesh/peers/{id}`) self-verify against open edges;
/// announce verifies identity only to accept a renewed membership.
pub(crate) fn is_custom_auth(method: &Method, path: &str) -> bool {
    if matches!(
        path,
        "/api/mesh/relay" | "/api/mesh/adopt/offer" | "/api/mesh/adopt/commit"
    ) {
        return true;
    }
    // Announce verifies its sender itself so an adopted peer can deliver a renewed membership.
    if method == Method::POST && path == "/api/mesh/announce" {
        return true;
    }
    if is_inbound_webhook_path(method, path) {
        return true;
    }
    if is_graph_event_path(method, path) {
        return true;
    }
    if path.starts_with("/api/mesh/relay-asset/") {
        return true;
    }
    // distinct from POST /peers/add-manual and the deeper /peers/{id}/… routes.
    method == Method::DELETE
        && path.starts_with("/api/mesh/peers/")
        && path.matches('/').count() == 4
}

fn is_graph_event_path(method: &Method, path: &str) -> bool {
    method == Method::POST
        && path
            .strip_prefix("/api/graph/events/")
            .is_some_and(|id| !id.is_empty() && !id.contains('/') && id.len() <= 128)
}

pub(crate) fn is_inbound_webhook_path(method: &Method, path: &str) -> bool {
    method == Method::POST
        && path == codeterm_service_core::service::web_access::PEBBLE_WEBHOOK_PATH
}

fn identify_caller(state: &AppState, auth_header: &str) -> Result<CallerIdentity, ()> {
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(());
    }

    if let Some(ref api_token) = state.config.lock().api.token {
        // differing byte and leaks the token prefix length via timing.
        if !api_token.is_empty()
            && codeterm_service_core::service::tokens::constant_time_eq(
                token.as_bytes(),
                api_token.as_bytes(),
            )
        {
            return Ok(CallerIdentity::Local);
        }
    }

    if let Ok(Some(svc_id)) = codeterm_service_core::service::push::validate_push(
        auth_header,
        &state.svc.tokens,
        &state.svc.services_store,
    ) {
        return Ok(CallerIdentity::ServicePush { service_id: svc_id });
    }

    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    let own_id = mesh.ident.identity.instance_id.clone();
    // identities serialized before public_key was added to MeshIdentity, which otherwise 401 every CLI call from outside a PTY (issue #1 from 1.1.9 SMOKE).
    let self_pubkey = mesh.ident.identity.public_key.clone().or_else(|| {
        mesh.ident
            .identity
            .private_key
            .as_ref()
            .and_then(|sk| codeterm_service_core::mesh::crown::public_key_from_private(sk).ok())
    });
    if let Some(pk) = self_pubkey {
        known_keys.insert(own_id.clone(), pk);
    }
    drop(mesh);

    match codeterm_service_core::mesh::crown::verify_signed_request(token, &known_keys) {
        Ok(sender_id) if sender_id == own_id => Ok(CallerIdentity::Local),
        Ok(sender_id) => {
            let (access_level, relay_protocol_version) = state
                .mesh
                .lock()
                .paired_peers
                .get(&sender_id)
                .map(|peer| (peer.access_level.clone(), peer.relay_protocol_version))
                .unwrap_or((AccessLevel::ReadOnly, 0));
            Ok(CallerIdentity::Peer {
                instance_id: sender_id,
                access_level,
                relay_protocol_version,
            })
        }
        Err(_) => Err(()),
    }
}

/// A paired peer whose signature is valid but whose traffic the account gate closed for the plan limit.
fn plan_limit_locked_peer(state: &AppState, token: &str) -> Option<String> {
    use codeterm_service_core::mesh::account_mesh::gate::{BlockReason, TrafficDecision};
    if token.is_empty() {
        return None;
    }
    let mesh = state.mesh.lock();
    let peer_id = codeterm_service_core::mesh::crown::verify_signed_request(
        token,
        &mesh.paired_identity_keys(),
    )
    .ok()?;
    (mesh.traffic_decision(&peer_id) == TrafficDecision::Blocked(BlockReason::PlanLimitLocked))
        .then_some(peer_id)
}

pub(crate) fn caller_peer_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<String> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    match identify_caller(state, auth).ok()? {
        CallerIdentity::Peer { instance_id, .. } => Some(instance_id),
        _ => None,
    }
}

pub(crate) fn is_local_caller(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    identify_caller(state, auth).is_ok_and(ownership_release_is_local)
}

fn ownership_release_is_local(caller: CallerIdentity) -> bool {
    matches!(caller, CallerIdentity::Local)
}

pub(crate) fn isolated_caller_peer_id(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Option<String> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    match identify_caller(state, auth).ok()? {
        CallerIdentity::Peer {
            instance_id,
            access_level: AccessLevel::AgentIsolated,
            ..
        } => Some(instance_id),
        _ => None,
    }
}

pub(crate) fn peer_owns_tab(
    peer_id: &str,
    tab: &codeterm_service_core::multiplexer::types::Tab,
) -> bool {
    tab.ownership
        .as_ref()
        .is_some_and(|ownership| ownership.peer_id == peer_id)
}

pub(crate) fn tab_owner_peer_id(state: &AppState, tab_id: &str) -> Option<String> {
    let mux = state.svc.mux.read();
    // Resolve exactly as the tab handlers do, so an alias cannot name a different tab.
    let resolved = mux.resolve_tab_id(tab_id)?;
    mux.get_tab(&resolved)
        .and_then(|tab| tab.ownership.as_ref())
        .map(|ownership| ownership.peer_id.clone())
}

/// Authorizes an authenticated peer request against live tab ownership; shared by
/// the signed-request middleware and the inbound mesh relay.
pub(crate) fn authorize_peer_request(
    state: &AppState,
    access: &AccessLevel,
    peer_id: &str,
    method: &Method,
    path: &str,
) -> bool {
    peer_access_allowed(access, peer_id, method, path, |tab_id| {
        tab_owner_peer_id(state, tab_id)
    })
}

pub(crate) fn caller_access_level(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<AccessLevel, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    match identify_caller(state, auth) {
        Ok(CallerIdentity::Local) => Ok(AccessLevel::Full),
        Ok(CallerIdentity::Peer { access_level, .. }) => Ok(access_level),
        Ok(CallerIdentity::ServicePush { .. }) => Err((
            StatusCode::FORBIDDEN,
            "Service push tokens cannot inspect mesh inventory".into(),
        )),
        Err(()) => Err((StatusCode::UNAUTHORIZED, "Invalid or missing token".into())),
    }
}

enum AuthOutcome {
    Allowed(Request, bool),
    Rejected(Response),
}

impl From<Response> for AuthOutcome {
    fn from(response: Response) -> Self {
        Self::Rejected(response)
    }
}

pub async fn auth_middleware(
    AxumState(state): AxumState<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let local_liveness = matches!(req.uri().path(), "/api/health" | "/api/mesh/ping")
        && req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|peer| peer.0.ip().is_loopback());
    if local_liveness {
        return next.run(req).await;
    }
    #[cfg(debug_assertions)]
    let queued = std::time::Instant::now();
    let mesh = state.mesh.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        #[cfg(debug_assertions)]
        let (started, route, queue_ms) = (
            std::time::Instant::now(),
            req.uri().path().to_owned(),
            queued.elapsed().as_millis(),
        );
        let result = authorize_request(state, req);
        #[cfg(debug_assertions)]
        if queue_ms > 100 || started.elapsed().as_millis() > 100 {
            log::debug!(
                "[REST-AUTH] {} queue={}ms execution={}ms",
                route,
                queue_ms,
                started.elapsed().as_millis()
            );
        }
        result
    })
    .await;
    match outcome {
        Ok(AuthOutcome::Allowed(req, false)) => next.run(req).await,
        Ok(AuthOutcome::Allowed(req, true)) => {
            let peer_id = match req.extensions().get::<CallerIdentity>() {
                Some(CallerIdentity::Peer { instance_id, .. }) => Some(instance_id.clone()),
                _ => None,
            };
            let response = super::peer_redaction::redact_peer_response(next.run(req).await).await;
            match peer_id {
                Some(peer_id) => {
                    crate::mesh_routes::account_link::fence_response(response, mesh, peer_id)
                }
                None => response,
            }
        }
        Ok(AuthOutcome::Rejected(response)) => response,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn authorize_request(state: AppState, mut req: Request) -> AuthOutcome {
    #[cfg(test)]
    if let Some(entered) = req
        .extensions()
        .get::<std::sync::Arc<tokio::sync::Notify>>()
    {
        entered.notify_one();
    }
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    let remote_address = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| *address);
    if !remote_network_allowed(state.config.lock().mesh.enabled, remote_address) {
        log::warn!(
            "[auth] Rejecting remote {} {} — mesh disabled",
            method,
            path
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Mesh networking is disabled",
        )
            .into_response()
            .into();
    }

    // /api/mesh/enabled (the local on/off switch — it MUST stay reachable to turn mesh back on; still token-gated, peers rejected below).
    if requires_mesh_network(&method, &path) {
        let mesh_enabled = state.config.lock().mesh.enabled;
        if !mesh_enabled {
            log::warn!(
                "[auth] Rejecting {} {} — mesh disabled in config",
                method,
                path
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Mesh networking is disabled",
            )
                .into_response()
                .into();
        }
    }

    if bearer_exempt(&method, &path) {
        return AuthOutcome::Allowed(req, false);
    }

    let auth = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let caller = identify_caller(&state, &auth);
    if let Ok(identity) = &caller {
        req.extensions_mut().insert(identity.clone());
    }
    match caller {
        Ok(CallerIdentity::Local) => AuthOutcome::Allowed(req, false),
        Ok(CallerIdentity::ServicePush { ref service_id }) => {
            if is_service_push_allowed(method.as_str(), &path) {
                AuthOutcome::Allowed(req, false)
            } else {
                log::warn!(
                    "[auth] Service push token {} denied on {} {} — push tokens are scoped to POST /api/tabs/<id>/send only",
                    service_id, method, path
                );
                (
                    StatusCode::FORBIDDEN,
                    "Service push tokens are scoped to POST /api/tabs/<id>/send",
                )
                    .into_response()
                    .into()
            }
        }
        Ok(CallerIdentity::Peer {
            ref instance_id,
            ref access_level,
            relay_protocol_version,
        }) => {
            if !state.config.lock().mesh.enabled {
                log::warn!(
                    "[auth] Rejecting peer {} on {} {} — mesh disabled in config",
                    instance_id,
                    method,
                    path
                );
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Mesh networking is disabled",
                )
                    .into_response()
                    .into();
            }
            record_peer_protocol(&state, instance_id, relay_protocol_version);
            match stored_protocol_answer(&method, &path, relay_protocol_version) {
                StoredProtocolAnswer::Proceed => {}
                StoredProtocolAnswer::EmptyGossip => {
                    return axum::Json(empty_gossip_snapshot()).into_response().into();
                }
                StoredProtocolAnswer::UpgradeRequired(message) => {
                    return (StatusCode::UPGRADE_REQUIRED, message)
                        .into_response()
                        .into();
                }
            }
            if authorize_peer_request(&state, access_level, instance_id, &method, &path) {
                AuthOutcome::Allowed(req, true)
            } else {
                log::warn!(
                    "[auth] Access denied for peer {} ({:?}) on {} {}",
                    instance_id,
                    access_level,
                    method,
                    path
                );
                (
                    StatusCode::FORBIDDEN,
                    "Access denied by access level policy",
                )
                    .into_response()
                    .into()
            }
        }
        Err(()) => {
            let presented = auth.strip_prefix("Bearer ").unwrap_or("");
            if let Some(peer_id) = plan_limit_locked_peer(&state, presented) {
                log::warn!("[auth-403] {method} {path} peer={peer_id} plan_limit_locked");
                return (
                    StatusCode::FORBIDDEN,
                    axum::Json(serde_json::json!({
                        "ok": false,
                        "error": {
                            "code": codeterm_service_core::service::billing::PLAN_LIMIT_LOCKED,
                            "peer": peer_id,
                            "message": codeterm_service_core::mesh::account_mesh::blocked_message(
                                codeterm_service_core::mesh::account_mesh::gate::BlockReason::PlanLimitLocked,
                            ),
                        }
                    })),
                )
                    .into_response()
                    .into();
            }
            let expected = state.config.lock().api.token.clone();
            if let Some((tab_id, event)) = tab_hook_target(&method, &path) {
                let reason = if presented.is_empty() {
                    "missing_token"
                } else {
                    "invalid_token"
                };
                log::warn!(
                    "[hook-auth] refused tab={} event={} reason={}",
                    tab_id,
                    event,
                    reason
                );
            }
            log::warn!(
                "[auth-401] {} {} presented[{}] expected[{}]",
                method,
                path,
                if presented.is_empty() {
                    "MISSING".to_string()
                } else {
                    token_fp(presented)
                },
                match expected {
                    Some(ref t) if !t.is_empty() => token_fp(t),
                    _ => "NONE".to_string(),
                }
            );
            (StatusCode::UNAUTHORIZED, "Invalid or missing token")
                .into_response()
                .into()
        }
    }
}

#[cfg(test)]
mod tests;
