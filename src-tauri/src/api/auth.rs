use axum::{
    extract::{Request, State as AxumState},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

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
            Ok(CallerIdentity::Peer {
                instance_id: sender_id,
                access_level,
            })
        }
        Err(_) => Err(()),
    }
}

fn check_access(access: &AccessLevel, method: &Method, path: &str) -> bool {
    match access {
        AccessLevel::Full => true,
        AccessLevel::ReadOnly => *method == Method::GET,
        AccessLevel::AgentIsolated => {
            if *method == Method::GET {
                return true;
            }
            let write_allowed = [
                "/send",
                "/exec",
                "/report",
                "/reports",
                "/respond",
                "/clear",
                "/describe",
                "/can-spawn",
                "/agent-type",
            ];
            if path.starts_with("/api/tabs/") && write_allowed.iter().any(|s| path.ends_with(s)) {
                return true;
            }
            path == "/api/agents/spawn" || path == "/api/batch" || path == "/api/notifications"
        }
        AccessLevel::Custom(perms) => {
            use crate::mesh::types::Permission;
            if perms.contains(&Permission::Admin) {
                return true;
            }
            if *method == Method::GET {
                let can_read = perms.contains(&Permission::ReadTabs)
                    || perms.contains(&Permission::ReadOutput);
                if can_read {
                    return true;
                }
            }
            if path.starts_with("/api/tabs/") {
                if path.ends_with("/send") && perms.contains(&Permission::SendInput) {
                    return true;
                }
                if (path.ends_with("/output") || path.ends_with("/text"))
                    && perms.contains(&Permission::ReadOutput)
                {
                    return true;
                }
                if perms.contains(&Permission::WriteTabs) {
                    return true;
                }
            }
            if path.starts_with("/api/agents/") && perms.contains(&Permission::SpawnAgents) {
                return true;
            }
            if path.starts_with("/api/workspaces") && perms.contains(&Permission::ManageWorkspaces)
            {
                return true;
            }
            if (path == "/api/fs/list-dir" || path == "/api/fs/mkdir")
                && (perms.contains(&Permission::ManageWorkspaces)
                    || perms.contains(&Permission::SpawnAgents))
            {
                return true;
            }
            if path.starts_with("/api/crons") && perms.contains(&Permission::ManageCrons) {
                return true;
            }
            false
        }
    }
}

pub async fn auth_middleware(
    AxumState(state): AxumState<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

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
        assert!(!is_service_push_allowed(
            "POST",
            "/api/tabs/abc/extra/send"
        ));
        assert!(!is_service_push_allowed("POST", "/api/tabs"));
    }
}
