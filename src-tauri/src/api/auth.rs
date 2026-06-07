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
    let Some(rest) = path.strip_prefix("/api/panes/") else {
        return false;
    };
    let Some(pane_id) = rest.strip_suffix("/send") else {
        return false;
    };
    !pane_id.is_empty() && !pane_id.contains('/')
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

fn is_custom_auth(path: &str) -> bool {
    path == "/api/mesh/relay"
        || path == "/api/mesh/introduce"
        || path.starts_with("/api/mesh/peers/") && !path.ends_with("/peers")
}

fn identify_caller(state: &AppState, auth_header: &str) -> Result<CallerIdentity, ()> {
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(());
    }

    if let Some(ref api_token) = state.config.lock().api.token {
        if !api_token.is_empty() && token == api_token {
            return Ok(CallerIdentity::Local);
        }
    }

    if let Ok(Some(svc_id)) = crate::services::push::validate_push(
        auth_header,
        &state.svc.tokens,
        &state.svc.services_store,
    ) {
        return Ok(CallerIdentity::ServicePush { service_id: svc_id });
    }

    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    let own_id = mesh.ident.identity.instance_id.clone();
    // Always register self as a known key. Prefer the cached pubkey, but fall back to deriving it from the private key on the fly — covers
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
            // Agents need to send commands, exec, spawn, report
            let write_allowed = [
                "/send",
                "/exec",
                "/report",
                "/reports",
                "/respond",
                "/clear",
                "/can-spawn",
                "/agent-type",
            ];
            if path.starts_with("/api/panes/") && write_allowed.iter().any(|s| path.ends_with(s)) {
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
                let can_read = perms.contains(&Permission::ReadPanes)
                    || perms.contains(&Permission::ReadOutput);
                if can_read {
                    return true;
                }
            }
            if path.starts_with("/api/panes/") {
                if path.ends_with("/send") && perms.contains(&Permission::SendInput) {
                    return true;
                }
                if (path.ends_with("/output") || path.ends_with("/text"))
                    && perms.contains(&Permission::ReadOutput)
                {
                    return true;
                }
                if perms.contains(&Permission::WritePanes) {
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
            if path == "/api/fs/list-dir"
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

    // Mesh-disabled gate: blocks /api/mesh/* except /api/mesh/ping (so paired peers detect us as down without retrying forever). Also blocks
    // /api/mesh/pair/* by design — re-enabling mesh requires a local config change, not a remote pair request.
    if path.starts_with("/api/mesh/") && path != "/api/mesh/ping" {
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

    // The /web static bundle is public by design: it is the same React code anyone can build from the repo. Every API call it makes is still
    // token-gated. Exact /web redirects to /web/ so the bundle's relative asset URLs resolve under the mount.
    if path == "/web" {
        return axum::response::Redirect::permanent("/web/").into_response();
    }
    if path.starts_with("/web/") {
        return next.run(req).await;
    }

    if PUBLIC_PATHS.iter().any(|p| path == *p) || is_custom_auth(&path) {
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
                    "[auth] Service push token {} denied on {} {} — push tokens are scoped to POST /api/panes/<id>/send only",
                    service_id, method, path
                );
                (
                    StatusCode::FORBIDDEN,
                    "Service push tokens are scoped to POST /api/panes/<id>/send",
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
        Err(()) => (StatusCode::UNAUTHORIZED, "Invalid or missing token").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "/api/panes/abc/send"
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
            "/api/panes"
        ));
        assert!(check_access(
            &AccessLevel::ReadOnly,
            &Method::GET,
            "/api/config"
        ));
        assert!(!check_access(
            &AccessLevel::ReadOnly,
            &Method::POST,
            "/api/panes/abc/send"
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
        assert!(check_access(&level, &Method::GET, "/api/panes"));
        assert!(check_access(&level, &Method::GET, "/api/config"));
        assert!(check_access(&level, &Method::POST, "/api/panes/abc/send"));
        assert!(check_access(&level, &Method::POST, "/api/panes/abc/exec"));
        assert!(check_access(&level, &Method::POST, "/api/agents/spawn"));
        assert!(check_access(&level, &Method::POST, "/api/panes/abc/report"));
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

        let level = AccessLevel::Custom(vec![Permission::ReadPanes, Permission::SendInput]);
        assert!(check_access(&level, &Method::GET, "/api/panes"));
        assert!(check_access(&level, &Method::POST, "/api/panes/abc/send"));
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
        assert!(is_custom_auth("/api/mesh/relay"));
        assert!(is_custom_auth("/api/mesh/introduce"));
        assert!(is_custom_auth("/api/mesh/peers/some-id"));
        assert!(!is_custom_auth("/api/mesh/peers"));
        assert!(!is_custom_auth("/api/mesh/status"));
        assert!(!is_custom_auth("/api/panes/abc/send"));
    }

    #[test]
    fn service_push_allowed_on_send() {
        assert!(is_service_push_allowed("POST", "/api/panes/abc123/send"));
        assert!(is_service_push_allowed("POST", "/api/panes/general/send"));
        assert!(is_service_push_allowed(
            "POST",
            "/api/panes/pane-with-dashes_and_underscores/send"
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
        assert!(!is_service_push_allowed("GET", "/api/panes/abc123/send"));
        assert!(!is_service_push_allowed("DELETE", "/api/panes/abc123/send"));
        assert!(!is_service_push_allowed("PUT", "/api/panes/abc123/send"));
    }

    #[test]
    fn service_push_rejected_on_pane_root() {
        assert!(!is_service_push_allowed("POST", "/api/panes/abc123"));
        assert!(!is_service_push_allowed("POST", "/api/panes/abc123/exec"));
        assert!(!is_service_push_allowed("POST", "/api/panes/abc123/clear"));
        assert!(!is_service_push_allowed("POST", "/api/panes//send"));
        assert!(!is_service_push_allowed(
            "POST",
            "/api/panes/abc/extra/send"
        ));
        assert!(!is_service_push_allowed("POST", "/api/panes"));
    }
}
