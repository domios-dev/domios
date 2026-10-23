use serde::{Deserialize, Serialize};
mod membership;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum OrdinaryTabKey {
    Local { tab_id: String },
    Remote { peer_id: String, tab_id: String },
}

impl OrdinaryTabKey {
    pub fn local(tab_id: impl Into<String>) -> Self {
        Self::Local {
            tab_id: tab_id.into(),
        }
    }

    pub fn remote(peer_id: impl Into<String>, tab_id: impl Into<String>) -> Self {
        Self::Remote {
            peer_id: peer_id.into(),
            tab_id: tab_id.into(),
        }
    }

    pub fn ordinary_id(&self) -> String {
        match self {
            Self::Local { tab_id } => tab_id.clone(),
            Self::Remote { peer_id, tab_id } => format!("mesh:{peer_id}/{tab_id}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemotePeerProjection {
    pub peer_id: String,
    pub peer_name: String,
    pub authenticated: bool,
    pub reachable: bool,
    pub availability: &'static str,
    pub tabs: Vec<serde_json::Value>,
    pub controllers: std::collections::HashMap<String, String>,
}

pub fn merge_ordinary_tab_projection(
    local: &[serde_json::Value],
    peers: &[RemotePeerProjection],
) -> Vec<serde_json::Value> {
    let mut projected = local.to_vec();
    for peer in peers.iter().filter(|peer| peer.authenticated) {
        for source in &peer.tabs {
            let Some(tab_id) = source.get("id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(controller_id) = peer.controllers.get(tab_id) else {
                continue;
            };
            let mut tab = source.clone();
            let Some(object) = tab.as_object_mut() else {
                continue;
            };
            let ordinary_id = OrdinaryTabKey::remote(&peer.peer_id, tab_id).ordinary_id();
            let remote_pty_id = object
                .get("pty_id")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            object.insert("id".into(), serde_json::json!(ordinary_id));
            object.insert("tab_id".into(), serde_json::json!(ordinary_id));
            object.insert("remote_pty_id".into(), remote_pty_id);
            if !object.get("pty_id").is_none_or(serde_json::Value::is_null) {
                object.insert("pty_id".into(), serde_json::json!(ordinary_id));
            }
            object.insert("remote_tab_id".into(), serde_json::json!(tab_id));
            object.insert("remote_peer_id".into(), serde_json::json!(peer.peer_id));
            object.insert("remote_peer_name".into(), serde_json::json!(peer.peer_name));
            object.insert("remote_peer".into(), serde_json::json!(peer.peer_name));
            object
                .entry("remote_availability")
                .or_insert_with(|| serde_json::json!(peer.availability));
            object.insert("parent_tab_id".into(), serde_json::json!(controller_id));
            object.insert("spawned_by_tab_id".into(), serde_json::json!(controller_id));
            if object.get("role").is_none_or(serde_json::Value::is_null) {
                object.insert(
                    "role".into(),
                    serde_json::json!(codeterm_types::agent::AgentRole::Worker),
                );
            }
            if let Some(controller) = local.iter().find(|candidate| {
                candidate.get("id").and_then(serde_json::Value::as_str)
                    == Some(controller_id.as_str())
            }) {
                let controller_title = controller
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(controller_id.as_str());
                object.insert(
                    "ownership".into(),
                    serde_json::json!({
                        "controller_title": controller_title,
                        "controller_tab_id": controller_id,
                        "peer_id": "",
                        "peer_name": "",
                    }),
                );
                // A remote viewer lives in its controller's local workspace; the
                // peer's workspace id is never a local assignment.
                for field in ["workspace_id", "team_id"] {
                    object.insert(
                        field.into(),
                        controller
                            .get(field)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    );
                }
            }
            membership::apply_member_placement(object);
            projected.push(tab);
        }
    }
    projected
}

pub fn scope_ordinary_tab_projection(
    tabs: Vec<serde_json::Value>,
    workspace_id: Option<&str>,
) -> Vec<serde_json::Value> {
    let Some(workspace_id) = workspace_id else {
        return tabs;
    };
    tabs.into_iter()
        .filter(|tab| {
            tab.get("workspace_id").and_then(serde_json::Value::as_str) == Some(workspace_id)
        })
        .collect()
}

type RemoteTabCache = std::collections::HashMap<String, Vec<serde_json::Value>>;

fn cache() -> &'static std::sync::Mutex<RemoteTabCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<RemoteTabCache>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(RemoteTabCache::new()))
}

pub async fn ordinary_tab_projection(
    mesh: &crate::MeshStateHandle,
    remote_workers: &crate::service::remote_workers::RemoteWorkerRegistry,
    remote_teams: &crate::service::remote_team::RemoteTeamStore,
    local: Vec<serde_json::Value>,
    workspace_id: Option<&str>,
) -> Result<Vec<serde_json::Value>, String> {
    let registered = remote_workers.snapshot();
    let members = remote_teams.members()?;
    let peers = {
        let manager = mesh.lock();
        manager
            .paired_peers
            .values()
            .map(|peer| {
                (
                    peer.instance_id.clone(),
                    peer.instance_name.clone(),
                    peer.online,
                    manager.client_for(&peer.instance_id),
                )
            })
            .collect::<Vec<_>>()
    };
    let paired_ids = peers
        .iter()
        .map(|(id, _, _, _)| id.clone())
        .collect::<std::collections::HashSet<_>>();
    cache()
        .lock()
        .unwrap()
        .retain(|peer, _| paired_ids.contains(peer));
    let mut remote = Vec::new();
    for (peer_id, peer_name, online, client) in peers {
        let peer_members = members
            .iter()
            .filter(|member| member.peer_id == peer_id)
            .cloned()
            .collect::<Vec<_>>();
        let mut controllers = registered
            .iter()
            .flat_map(|(controller, workers)| {
                workers
                    .iter()
                    .filter(|worker| worker.peer == peer_id || worker.peer == peer_name)
                    .map(|worker| (worker.remote_tab.clone(), controller.clone()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        for member in &peer_members {
            if member.finalized {
                controllers.remove(&member.tab_id);
            } else {
                controllers.insert(member.tab_id.clone(), member.orchestrator_tab_id.clone());
            }
        }
        if controllers.is_empty() {
            continue;
        }
        let mut fetched: Option<Vec<serde_json::Value>> = None;
        if online {
            if let Some(client) = client {
                let command = crate::mesh::types::RelayCommand {
                    method: "GET".into(),
                    path: "/api/tabs?local_only=true".into(),
                    body: serde_json::Value::Null,
                    delivery_id: None,
                };
                if let Ok(response) = client.send_relay(&command).await {
                    if response.ok {
                        fetched = response
                            .data
                            .and_then(|value| value.as_array().cloned())
                            .map(|tabs| {
                                tabs.into_iter()
                                    .filter(|tab| {
                                        tab.get("id")
                                            .and_then(serde_json::Value::as_str)
                                            .is_some_and(|id| controllers.contains_key(id))
                                    })
                                    .collect()
                            });
                    }
                }
            }
        }
        if let Some(tabs) = fetched.as_ref() {
            cache()
                .lock()
                .unwrap()
                .insert(peer_id.clone(), tabs.clone());
        }
        let fetched_ok = fetched.is_some();
        let tabs = fetched
            .or_else(|| cache().lock().unwrap().get(&peer_id).cloned())
            .unwrap_or_default();
        let tabs = tabs
            .into_iter()
            .filter(|tab| {
                let id = tab
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let session = tab
                    .get("agent_session_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let provider = tab
                    .get("agent_type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                !remote_teams.was_finalized(&peer_id, id, provider, session)
            })
            .collect();
        let tabs = membership::member_sources(tabs, &peer_members);
        if tabs.is_empty() {
            continue;
        }
        remote.push(RemotePeerProjection {
            peer_id,
            peer_name,
            authenticated: true,
            reachable: online && fetched_ok,
            availability: if !online {
                "reconnecting"
            } else if fetched_ok {
                "available"
            } else {
                "unavailable"
            },
            tabs,
            controllers,
        });
    }
    let member_peers = members
        .iter()
        .map(|member| member.peer_id.clone())
        .collect::<std::collections::HashSet<_>>();
    for peer_id in member_peers.difference(&paired_ids) {
        let peer_members = members
            .iter()
            .filter(|member| &member.peer_id == peer_id)
            .cloned()
            .collect::<Vec<_>>();
        let controllers = peer_members
            .iter()
            .filter(|member| !member.finalized)
            .map(|member| (member.tab_id.clone(), member.orchestrator_tab_id.clone()))
            .collect();
        remote.push(RemotePeerProjection {
            peer_id: peer_id.clone(),
            peer_name: peer_id.clone(),
            authenticated: true,
            reachable: false,
            availability: "unavailable",
            tabs: membership::member_sources(Vec::new(), &peer_members),
            controllers,
        });
    }
    Ok(scope_ordinary_tab_projection(
        merge_ordinary_tab_projection(&local, &remote),
        workspace_id,
    ))
}

#[cfg(test)]
mod tests;
