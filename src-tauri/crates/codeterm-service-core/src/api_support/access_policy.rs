use axum::http::Method;

use crate::mesh::types::{AccessLevel, Permission};

/// Decision-model tokens are machine-local credentials, like secrets.
const SUPPORTER_TOKEN_PATH: &str = "/api/supporter/token";

pub fn request_path(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

fn tab_id_from_scoped_path(path: &str) -> Option<&str> {
    let rest = request_path(path).strip_prefix("/api/tabs/")?;
    let (tab_id, _) = rest.split_once('/')?;
    (!tab_id.is_empty()).then_some(tab_id)
}

/// The one peer authorization decision for direct and relayed requests alike.
/// `tab_owner` resolves a tab's stored owner peer id; it runs only for AgentIsolated.
pub fn peer_access_allowed(
    access: &AccessLevel,
    peer_id: &str,
    method: &Method,
    path: &str,
    tab_owner: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let path = request_path(path);
    if path == "/api/agents/reparent"
        || (path.starts_with("/api/teams/") && path.ends_with("/adopt-remote"))
    {
        return false;
    }
    if !check_access(access, method, path) {
        return false;
    }
    *access != AccessLevel::AgentIsolated
        || tab_id_from_scoped_path(path)
            .is_none_or(|tab_id| tab_owner(tab_id).as_deref() == Some(peer_id))
}

pub fn is_preview_read(method: &Method, path: &str) -> bool {
    if *method != Method::POST {
        return false;
    }
    let Some(rest) = request_path(path).strip_prefix("/api/tabs/") else {
        return false;
    };
    let Some((tab, suffix)) = rest.split_once('/') else {
        return false;
    };
    !tab.is_empty() && matches!(suffix, "preview/read" | "preview/check")
}

pub fn check_access(access: &AccessLevel, method: &Method, path: &str) -> bool {
    let path = request_path(path);
    if path.starts_with("/api/analytics/")
        || path.starts_with("/api/secrets/")
        || path == "/api/mesh/debug/sign"
        || path == SUPPORTER_TOKEN_PATH
    {
        return false;
    }
    if *method == Method::POST && path == "/api/mesh/announce" {
        return matches!(access, AccessLevel::Full);
    }
    if *method == Method::POST && path == "/api/mesh/sync" {
        return matches!(
            access,
            AccessLevel::Full | AccessLevel::ReadOnly | AccessLevel::AgentIsolated
        ) || matches!(access, AccessLevel::Custom(perms) if perms.contains(&Permission::Admin));
    }
    if is_preview_read(method, path) {
        return match access {
            AccessLevel::Full | AccessLevel::ReadOnly | AccessLevel::AgentIsolated => true,
            AccessLevel::Custom(perms) => perms.iter().any(|permission| {
                matches!(
                    permission,
                    Permission::Admin | Permission::ReadTabs | Permission::ReadOutput
                )
            }),
        };
    }
    match access {
        AccessLevel::Full => true,
        AccessLevel::ReadOnly => *method == Method::GET,
        AccessLevel::AgentIsolated => {
            if *method == Method::GET && path == "/api/tabs" {
                return true;
            }
            if path == "/api/agents/spawn" && *method == Method::POST {
                return true;
            }
            if tab_id_from_scoped_path(path).is_none() {
                return false;
            }
            if *method == Method::GET && path.ends_with("/text") {
                return true;
            }
            *method == Method::POST
                && [
                    "/send",
                    "/attachments",
                    "/exec",
                    "/respond",
                    "/report",
                    "/clear",
                ]
                .iter()
                .any(|suffix| path.ends_with(suffix))
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
mod tests;
