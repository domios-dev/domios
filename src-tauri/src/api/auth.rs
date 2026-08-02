use axum::{
    extract::{ConnectInfo, Request, State as AxumState},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use super::routes::AppState;
use super::access_policy::check_access;
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

const PUBLIC_PATHS: &[&str] = &[
    "/api/health",
    "/api/providers",
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

/// Paths whose handler does its OWN authentication (a mesh-signed token, not the
/// bearer API token) and must therefore skip the bearer-token middleware. This
/// is an EXACT allowlist on purpose: an earlier `starts_with("/api/mesh/peers/")`
/// prefix match exempted every peer sub-route (services, approve, add-manual,
/// addr, …) whose handlers only call the no-op `check_auth`, leaving them fully
/// unauthenticated — an anonymous caller on the network could self-approve a
/// pairing. Only `relay`, `introduce`, and the unpair handler
/// (`DELETE /api/mesh/peers/{id}`) self-verify.
fn is_custom_auth(method: &Method, path: &str) -> bool {
    if path == "/api/mesh/relay" || path == "/api/mesh/introduce" {
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
        log::warn!("[auth] Rejecting remote {} {} — mesh disabled", method, path);
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
            if check_access(access_level, &method, &path) {
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
    use super::*;

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
        assert!(check_access(&level, &Method::GET, "/api/config"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/send"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/exec"));
        assert!(check_access(&level, &Method::POST, "/api/agents/spawn"));
        assert!(check_access(&level, &Method::POST, "/api/tabs/abc/report"));
        assert!(!check_access(&level, &Method::POST, "/api/config"));
        assert!(!check_access(&level, &Method::DELETE, "/api/crons/123"));
        assert!(!check_access(
            &level,
            &Method::POST,
            "/api/mesh/broadcast/start"
        ));
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
        assert!(
            PUBLIC_PATHS.contains(&"/api/providers"),
            "plugin verification needs unauthenticated provider liveness checks"
        );
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
