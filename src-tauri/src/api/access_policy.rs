use axum::http::Method;

use crate::mesh::types::{AccessLevel, Permission};

fn is_mesh_control(method: &Method, path: &str) -> bool {
    *method == Method::POST && matches!(path, "/api/mesh/announce" | "/api/mesh/sync")
}

pub(crate) fn check_access(access: &AccessLevel, method: &Method, path: &str) -> bool {
    let bare_path = path.split('?').next().unwrap_or(path);
    if bare_path.starts_with("/api/secrets/") || bare_path == "/api/mesh/debug/sign" {
        return false;
    }
    if is_mesh_control(method, path) {
        return true;
    }
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
                "/attachments",
            ];
            if path.starts_with("/api/tabs/") && write_allowed.iter().any(|s| path.ends_with(s)) {
                return true;
            }
            path == "/api/agents/spawn" || path == "/api/batch" || path == "/api/notifications"
        }
        AccessLevel::Custom(perms) => {
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
                if (path.ends_with("/send") || path.ends_with("/attachments"))
                    && perms.contains(&Permission::SendInput)
                {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_allows_reads_and_mesh_control_only() {
        let access = AccessLevel::ReadOnly;
        assert!(check_access(&access, &Method::GET, "/api/tabs"));
        assert!(check_access(&access, &Method::POST, "/api/mesh/announce"));
        assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
        assert!(!check_access(&access, &Method::POST, "/api/tabs/abc/send"));
        assert!(!check_access(&access, &Method::PUT, "/api/config"));
        assert!(!check_access(&access, &Method::PATCH, "/api/config"));
        assert!(!check_access(&access, &Method::DELETE, "/api/crons/123"));
    }

    #[test]
    fn agent_isolated_keeps_agent_scope() {
        let access = AccessLevel::AgentIsolated;
        assert!(check_access(&access, &Method::GET, "/api/tabs"));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/send"));
        assert!(check_access(&access, &Method::POST, "/api/agents/spawn"));
        assert!(!check_access(&access, &Method::POST, "/api/config"));
        assert!(!check_access(&access, &Method::DELETE, "/api/crons/123"));
    }

    #[test]
    fn custom_permissions_remain_explicit() {
        let access = AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]);
        assert!(check_access(&access, &Method::GET, "/api/tabs"));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/send"));
        assert!(!check_access(&access, &Method::POST, "/api/agents/spawn"));
        assert!(!check_access(&access, &Method::POST, "/api/config"));
    }

    #[test]
    fn peer_local_secrets_are_never_authorized() {
        for access in [
            AccessLevel::Full,
            AccessLevel::ReadOnly,
            AccessLevel::AgentIsolated,
            AccessLevel::Custom(vec![Permission::Admin]),
        ] {
            assert!(!check_access(&access, &Method::GET, "/api/secrets/items"));
            assert!(!check_access(&access, &Method::GET, "/api/mesh/debug/sign"));
        }
    }
}
