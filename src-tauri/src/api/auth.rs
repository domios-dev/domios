use axum::{
    extract::{ConnectInfo, Request, State as AxumState},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use super::access_policy::peer_access_allowed;
use super::routes::AppState;
use crate::mesh::types::AccessLevel;

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

pub(crate) const PUBLIC_PATHS: &[&str] = &[
    "/api/health",
    "/api/mesh/ping",
    "/api/mesh/pair/request",
    "/api/mesh/pair/accept",
    "/api/mesh/pair/confirm",
    "/api/mesh/pair/reject",
];

fn peer_protocol_allowed(version: u16, path: &str) -> bool {
    version == crate::mesh::types::RELAY_PROTOCOL_VERSION
        || matches!(path, "/api/mesh/announce" | "/api/mesh/sync")
}

fn remote_network_allowed(mesh_enabled: bool, address: Option<SocketAddr>) -> bool {
    mesh_enabled || address.is_none_or(|address| address.ip().is_loopback())
}

/// Paths whose handlers authenticate independently and must skip bearer middleware.
/// This is an EXACT allowlist on purpose: an earlier `starts_with("/api/mesh/peers/")`
/// prefix match exempted every peer sub-route (services, approve, add-manual,
/// addr, …) whose handlers only call the no-op `check_auth`, leaving them fully
/// unauthenticated — an anonymous caller on the network could self-approve a
/// pairing. Only `relay`, `introduce`, and the unpair handler
/// (`DELETE /api/mesh/peers/{id}`) self-verify.
pub(crate) fn is_custom_auth(method: &Method, path: &str) -> bool {
    if path == "/api/mesh/relay" || path == "/api/mesh/introduce" {
        return true;
    }
    if is_inbound_webhook_path(method, path) {
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

pub(crate) fn is_inbound_webhook_path(method: &Method, path: &str) -> bool {
    method == Method::POST && path == super::inbound_webhook::PEBBLE_WEBHOOK_PATH
}

/// Non-leaking token fingerprint for diagnostics: byte length + a short
/// non-cryptographic hash. Never logs the token itself.
pub(crate) fn token_fp(token: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut h);
    format!("len={} fp={:08x}", token.len(), (h.finish() as u32))
}

fn identify_caller(state: &AppState, auth_header: &str) -> Result<CallerIdentity, ()> {
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(());
    }

    if let Some(ref api_token) = state.config.lock().api.token {
        // differing byte and leaks the token prefix length via timing.
        if !api_token.is_empty()
            && crate::service::tokens::constant_time_eq(token.as_bytes(), api_token.as_bytes())
        {
            return Ok(CallerIdentity::Local);
        }
    }

    if let Ok(Some(svc_id)) = crate::service::push::validate_push(
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
            .and_then(|sk| crate::mesh::crown::public_key_from_private(sk).ok())
    });
    if let Some(pk) = self_pubkey {
        known_keys.insert(own_id.clone(), pk);
    }
    let paired_peers = mesh.paired_peers.clone();
    drop(mesh);

    match crate::mesh::crown::verify_signed_request(token, &known_keys) {
        Ok(sender_id) if sender_id == own_id => Ok(CallerIdentity::Local),
        Ok(sender_id) => {
            let access_level = paired_peers
                .get(&sender_id)
                .map(|p| p.access_level.clone())
                .unwrap_or(AccessLevel::ReadOnly);
            let relay_protocol_version = paired_peers
                .get(&sender_id)
                .map(|peer| peer.relay_protocol_version)
                .unwrap_or(0);
            Ok(CallerIdentity::Peer {
                instance_id: sender_id,
                access_level,
                relay_protocol_version,
            })
        }
        Err(_) => Err(()),
    }
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

pub(crate) fn peer_owns_tab(peer_id: &str, tab: &crate::multiplexer::types::Tab) -> bool {
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

pub async fn auth_middleware(
    AxumState(state): AxumState<AppState>,
    req: Request,
    next: Next,
) -> Response {
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
            .into_response();
    }

    // /api/mesh/enabled (the local on/off switch — it MUST stay reachable to turn mesh back on; still token-gated, peers rejected below).
    if path.starts_with("/api/mesh/") && path != "/api/mesh/ping" && path != "/api/mesh/enabled" {
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
                .into_response();
        }
    }

    if !path.starts_with("/api/") {
        return next.run(req).await;
    }

    if path.starts_with("/api/plugins/") && path.contains("/ui/") {
        return next.run(req).await;
    }

    if PUBLIC_PATHS.iter().any(|p| path == *p) || is_custom_auth(&method, &path) {
        return next.run(req).await;
    }

    let auth = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    match identify_caller(&state, &auth) {
        Ok(CallerIdentity::Local) => next.run(req).await,
        Ok(CallerIdentity::ServicePush { ref service_id }) => {
            if is_service_push_allowed(method.as_str(), &path) {
                next.run(req).await
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
                    .into_response();
            }
            if !peer_protocol_allowed(relay_protocol_version, &path) {
                log::warn!(
                    "[auth] Rejecting peer {} with relay protocol {} on {} {}",
                    instance_id,
                    relay_protocol_version,
                    method,
                    path
                );
                return (
                    StatusCode::UPGRADE_REQUIRED,
                    "Incompatible Mesh relay protocol; update CodeTerm",
                )
                    .into_response();
            }
            if authorize_peer_request(&state, access_level, instance_id, &method, &path) {
                next.run(req).await
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
            }
        }
        Err(()) => {
            let presented = auth.strip_prefix("Bearer ").unwrap_or("");
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
            (StatusCode::UNAUTHORIZED, "Invalid or missing token").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::access_policy::check_access;
    use super::*;

    #[test]
    fn hook_auth_context_only_matches_binding_events() {
        assert_eq!(
            tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/session_start"),
            Some(("tab-1", "session_start")),
        );
        assert_eq!(
            tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/provider_event"),
            Some(("tab-1", "provider_event")),
        );
        assert_eq!(
            tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/stop"),
            Some(("tab-1", "stop")),
        );
        assert_eq!(
            tab_hook_target(&Method::GET, "/api/tabs/tab-1/hook/stop"),
            None,
        );
        assert_eq!(
            tab_hook_target(&Method::POST, "/api/tabs/tab-1/hook/permission"),
            None,
        );
    }

    #[test]
    fn custom_auth_only_exempts_self_verifying_paths() {
        // Self-verifying: skip the bearer middleware.
        assert!(is_custom_auth(&Method::POST, "/api/mesh/relay"));
        assert!(is_custom_auth(&Method::POST, "/api/mesh/introduce"));
        assert!(is_custom_auth(&Method::DELETE, "/api/mesh/peers/abc123")); // unpair
                                                                            // Relayed plugin-UI asset proxy: iframe src, handler self-verifies query token.
        assert!(is_custom_auth(
            &Method::GET,
            "/api/mesh/relay-asset/peer1/plugins/foo/ui/index.html"
        ));
        // The bare route without a peer segment is not the proxy — not exempt.
        assert!(!is_custom_auth(&Method::GET, "/api/mesh/relay-asset"));

        // therefore fully unauthenticated. They must NOT be custom-auth.
        assert!(!is_custom_auth(
            &Method::GET,
            "/api/mesh/peers/abc123/services"
        ));
        assert!(!is_custom_auth(&Method::POST, "/api/mesh/peers/add-manual"));
        assert!(!is_custom_auth(
            &Method::POST,
            "/api/mesh/peers/approve/abc123"
        ));
        assert!(!is_custom_auth(
            &Method::POST,
            "/api/mesh/peers/reject/abc123"
        ));
        assert!(!is_custom_auth(
            &Method::POST,
            "/api/mesh/peers/abc123/addr"
        ));
        // Unpair must be DELETE — a GET on the same path is not exempt.
        assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers/abc123"));

        assert!(is_inbound_webhook_path(
            &Method::POST,
            "/api/plugins/pebble/webhook"
        ));
        assert!(is_custom_auth(&Method::POST, "/api/plugins/pebble/webhook"));
        assert!(!is_inbound_webhook_path(
            &Method::GET,
            "/api/plugins/pebble/webhook"
        ));
        assert!(!is_inbound_webhook_path(
            &Method::POST,
            "/api/plugins/pebble/webhook/extra"
        ));
        assert!(!is_inbound_webhook_path(
            &Method::POST,
            "/api/plugins/pebble/ui/webhook"
        ));
        assert!(!is_inbound_webhook_path(
            &Method::POST,
            "/api/plugins/other/webhook"
        ));
        assert!(!is_custom_auth(&Method::POST, "/api/plugins/other/webhook"));
        assert!(!is_custom_auth(
            &Method::POST,
            "/api/plugins/pebble/webhook/extra"
        ));
    }

    #[test]
    fn old_peer_protocol_can_only_bootstrap_compatibility_state() {
        assert!(peer_protocol_allowed(0, "/api/mesh/announce"));
        assert!(peer_protocol_allowed(0, "/api/mesh/sync"));
        assert!(!peer_protocol_allowed(0, "/api/events/stream"));
        assert!(!peer_protocol_allowed(0, "/api/config"));
        assert!(peer_protocol_allowed(
            crate::mesh::types::RELAY_PROTOCOL_VERSION,
            "/api/events/stream"
        ));
    }

    #[test]
    fn full_access_allows_everything() {
        assert!(check_access(
            &AccessLevel::Full,
            &Method::GET,
            "/api/config"
        ));
        assert!(check_access(
            &AccessLevel::Full,
            &Method::POST,
            "/api/tabs/abc/send"
        ));
        assert!(check_access(
            &AccessLevel::Full,
            &Method::DELETE,
            "/api/crons/123"
        ));
    }

    #[test]
    fn read_only_blocks_writes() {
        assert!(check_access(
            &AccessLevel::ReadOnly,
            &Method::GET,
            "/api/tabs"
        ));
        assert!(check_access(
            &AccessLevel::ReadOnly,
            &Method::GET,
            "/api/config"
        ));
        assert!(!check_access(
            &AccessLevel::ReadOnly,
            &Method::POST,
            "/api/tabs/abc/send"
        ));
        assert!(!check_access(
            &AccessLevel::ReadOnly,
            &Method::DELETE,
            "/api/crons/123"
        ));
    }

    #[test]
    fn agent_isolated_allows_agent_ops() {
        let level = AccessLevel::AgentIsolated;
        assert!(check_access(&level, &Method::GET, "/api/tabs"));
        assert!(check_access(&level, &Method::GET, "/api/tabs/abc/text"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/send"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/exec"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/respond"));
        assert!(check_access(&level, &Method::POST, "/api/agents/spawn"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/report"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/clear"));
        assert!(!check_access(&level, &Method::GET, "/api/config"));
        assert!(!check_access(&level, &Method::GET, "/api/mem/search"));
        assert!(!check_access(&level, &Method::GET, "/api/notifications"));
        assert!(!check_access(&level, &Method::GET, "/api/workspaces"));
        assert!(!check_access(&level, &Method::POST, "/api/config"));
        assert!(!check_access(&level, &Method::DELETE, "/api/crons/123"));
        assert!(!check_access(
            &level,
            &Method::POST,
            "/api/mesh/broadcast/start"
        ));
    }

    #[test]
    fn agent_isolated_limited_to_spawned_tabs() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let tab_id = mux.snapshot().workspaces[0].tabs[0].clone();
        mux.get_tab_mut(&tab_id).unwrap().ownership = Some(
            codeterm_types::pty::RemoteOwnership {
                controller_title: "remote".into(),
                controller_tab_id: None,
                peer_id: "peer-a".into(),
                peer_name: "Peer A".into(),
                team: None,
            },
        );
        let level = AccessLevel::AgentIsolated;
        for (method, path) in [
            (Method::GET, "/api/tabs/abc/text"),
            (Method::POST, "/api/tabs/abc/send"),
            (Method::POST, "/api/tabs/abc/exec"),
            (Method::POST, "/api/tabs/abc/prompt/respond"),
            (Method::POST, "/api/tabs/abc/report"),
            (Method::POST, "/api/tabs/abc/prompt/clear"),
            (Method::POST, "/api/tabs/abc/attachments"),
        ] {
            let owner_is = |owner: &'static str| {
                peer_access_allowed(&level, "peer-a", &method, path, |_| Some(owner.into()))
            };
            assert!(owner_is("peer-a"));
            assert!(!owner_is("peer-b"));
        }
        assert!(!peer_access_allowed(
            &level,
            "peer-a",
            &Method::POST,
            "/api/tabs/abc/send",
            |_| None
        ));
        assert!(peer_owns_tab("peer-a", mux.get_tab(&tab_id).unwrap()));
    }

    #[test]
    fn custom_permissions() {
        use crate::mesh::types::Permission;

        let level = AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]);
        assert!(check_access(&level, &Method::GET, "/api/tabs"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/send"));
        assert!(!check_access(&level, &Method::POST, "/api/agents/spawn"));
        assert!(!check_access(&level, &Method::POST, "/api/config"));

        let admin = AccessLevel::Custom(vec![Permission::Admin]);
        assert!(check_access(&admin, &Method::POST, "/api/config"));
        assert!(check_access(&admin, &Method::DELETE, "/api/crons/123"));
    }

    #[test]
    fn public_paths_recognized() {
        for p in PUBLIC_PATHS {
            assert!(PUBLIC_PATHS.iter().any(|pp| pp == p));
        }
        assert!(!PUBLIC_PATHS.contains(&"/api/providers"));
    }

    #[test]
    fn remote_network_follows_mesh_toggle() {
        let local = "127.0.0.1:7685".parse().unwrap();
        let remote = "192.0.2.10:7685".parse().unwrap();

        assert!(remote_network_allowed(false, None));
        assert!(remote_network_allowed(false, Some(local)));
        assert!(!remote_network_allowed(false, Some(remote)));
        assert!(remote_network_allowed(true, Some(remote)));
    }

    #[test]
    fn custom_auth_paths() {
        assert!(is_custom_auth(&Method::POST, "/api/mesh/relay"));
        assert!(is_custom_auth(&Method::POST, "/api/mesh/introduce"));
        // Unpair self-verifies; only the DELETE on the bare id is exempt.
        assert!(is_custom_auth(&Method::DELETE, "/api/mesh/peers/some-id"));
        assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers/some-id"));
        assert!(!is_custom_auth(&Method::GET, "/api/mesh/peers"));
        assert!(!is_custom_auth(&Method::GET, "/api/mesh/status"));
        assert!(!is_custom_auth(&Method::POST, "/api/tabs/abc/send"));
    }

    #[test]
    fn service_push_allowed_on_send() {
        assert!(is_service_push_allowed("POST", "/api/tabs/abc123/send"));
        assert!(is_service_push_allowed("POST", "/api/tabs/general/send"));
        assert!(is_service_push_allowed(
            "POST",
            "/api/tabs/tab-with-dashes_and_underscores/send"
        ));
    }

    #[test]
    fn service_push_rejected_on_config() {
        assert!(!is_service_push_allowed("POST", "/api/config"));
    }

    #[test]
    fn service_push_rejected_on_spawn() {
        assert!(!is_service_push_allowed("POST", "/api/agents/spawn"));
    }

    #[test]
    fn service_push_rejected_on_get() {
        assert!(!is_service_push_allowed("GET", "/api/tabs/abc123/send"));
        assert!(!is_service_push_allowed("DELETE", "/api/tabs/abc123/send"));
        assert!(!is_service_push_allowed("PUT", "/api/tabs/abc123/send"));
    }

    #[test]
    fn service_push_rejected_on_tab_root() {
        assert!(!is_service_push_allowed("POST", "/api/tabs/abc123"));
        assert!(!is_service_push_allowed("POST", "/api/tabs/abc123/exec"));
        assert!(!is_service_push_allowed("POST", "/api/tabs/abc123/clear"));
        assert!(!is_service_push_allowed("POST", "/api/tabs//send"));
        assert!(!is_service_push_allowed("POST", "/api/tabs/abc/extra/send"));
        assert!(!is_service_push_allowed("POST", "/api/tabs"));
    }
}
