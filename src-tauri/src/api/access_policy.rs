use axum::http::Method;

use crate::mesh::types::{AccessLevel, Permission};

pub(crate) fn request_path(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

fn tab_id_from_scoped_path(path: &str) -> Option<&str> {
    let rest = request_path(path).strip_prefix("/api/tabs/")?;
    let (tab_id, _) = rest.split_once('/')?;
    (!tab_id.is_empty()).then_some(tab_id)
}

/// The one peer authorization decision for direct and relayed requests alike.
/// `tab_owner` resolves a tab's stored owner peer id; it runs only for AgentIsolated.
pub(crate) fn peer_access_allowed(
    access: &AccessLevel,
    peer_id: &str,
    method: &Method,
    path: &str,
    tab_owner: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let path = request_path(path);
    if !check_access(access, method, path) {
        return false;
    }
    *access != AccessLevel::AgentIsolated
        || tab_id_from_scoped_path(path)
            .is_none_or(|tab_id| tab_owner(tab_id).as_deref() == Some(peer_id))
}

pub(crate) fn check_access(access: &AccessLevel, method: &Method, path: &str) -> bool {
    let path = request_path(path);
    if path.starts_with("/api/secrets/") || path == "/api/mesh/debug/sign" {
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
mod tests {
    use super::*;

    #[test]
    fn read_only_allows_reads_and_mesh_control_only() {
        let access = AccessLevel::ReadOnly;
        assert!(check_access(&access, &Method::GET, "/api/tabs"));
        assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
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
        assert!(check_access(&access, &Method::GET, "/api/tabs/abc/text"));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/send"));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/exec"));
        assert!(check_access(
            &access,
            &Method::POST,
            "/api/tabs/abc/respond"
        ));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/report"));
        assert!(check_access(&access, &Method::POST, "/api/tabs/abc/clear"));
        assert!(check_access(&access, &Method::POST, "/api/agents/spawn"));
        assert!(!check_access(&access, &Method::GET, "/api/config"));
        assert!(!check_access(&access, &Method::GET, "/api/mem/search"));
        assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
        assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
        assert!(!check_access(&access, &Method::POST, "/api/batch"));
        assert!(!check_access(&access, &Method::POST, "/api/config"));
        assert!(!check_access(&access, &Method::DELETE, "/api/crons/123"));
    }

    fn owned_by(access: &AccessLevel, method: &Method, path: &str, owner: Option<&str>) -> bool {
        peer_access_allowed(access, "peer-a", method, path, |_| {
            owner.map(str::to_string)
        })
    }

    #[test]
    fn query_string_does_not_change_the_access_decision() {
        let levels = [
            AccessLevel::Full,
            AccessLevel::ReadOnly,
            AccessLevel::AgentIsolated,
            AccessLevel::Custom(vec![Permission::ReadTabs, Permission::SendInput]),
        ];
        let requests = [
            (Method::GET, "/api/tabs"),
            (Method::GET, "/api/tabs/abc/text"),
            (Method::POST, "/api/tabs/abc/send"),
            (Method::POST, "/api/agents/spawn"),
            (Method::POST, "/api/mesh/announce"),
            (Method::GET, "/api/config"),
            (Method::GET, "/api/secrets/items"),
        ];
        for access in &levels {
            for (method, path) in &requests {
                let with_query = format!("{path}?agents=false&formatted=false");
                for owner in [Some("peer-a"), Some("peer-b"), None] {
                    assert_eq!(
                        owned_by(access, method, path, owner),
                        owned_by(access, method, &with_query, owner),
                        "{access:?} {method} {with_query}"
                    );
                }
            }
        }
        let isolated = AccessLevel::AgentIsolated;
        let listing = "/api/tabs?agents=false";
        assert!(owned_by(&isolated, &Method::GET, listing, None));
        let own_text = "/api/tabs/abc/text?formatted=false";
        assert!(owned_by(&isolated, &Method::GET, own_text, Some("peer-a")));
    }

    #[test]
    fn agent_isolated_owner_lookup_targets_the_path_tab() {
        let isolated = AccessLevel::AgentIsolated;
        for (method, path) in [
            (Method::POST, "/api/tabs/foreign/send"),
            (Method::POST, "/api/tabs/foreign/exec"),
            (Method::POST, "/api/tabs/foreign/respond"),
            (Method::POST, "/api/tabs/foreign/prompt/respond"),
            (Method::POST, "/api/tabs/foreign/attachments"),
            (Method::GET, "/api/tabs/foreign/text?formatted=false"),
        ] {
            let mut asked = None;
            let allowed = peer_access_allowed(&isolated, "peer-a", &method, path, |tab| {
                asked = Some(tab.to_string());
                Some("peer-b".to_string())
            });
            assert!(!allowed, "{method} {path}");
            assert_eq!(asked.as_deref(), Some("foreign"), "{method} {path}");
            assert!(!owned_by(&isolated, &method, path, None));
            assert!(owned_by(&isolated, &method, path, Some("peer-a")));
        }
    }

    #[test]
    fn full_peer_access_never_consults_tab_ownership() {
        let allowed = peer_access_allowed(
            &AccessLevel::Full,
            "peer-a",
            &Method::POST,
            "/api/tabs/foreign/send",
            |_| panic!("owner lookup is AgentIsolated-only"),
        );
        assert!(allowed);
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

    #[test]
    fn mesh_control_denied_below_full() {
        for access in [AccessLevel::ReadOnly, AccessLevel::AgentIsolated] {
            assert!(!check_access(&access, &Method::POST, "/api/mesh/announce"));
            assert!(check_access(&access, &Method::POST, "/api/mesh/sync"));
        }
        assert!(check_access(
            &AccessLevel::Full,
            &Method::POST,
            "/api/mesh/announce"
        ));
    }
}
