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

struct PeerSource {
    peer_id: String,
    peer_name: String,
    online: bool,
    client: Option<crate::mesh::transport::PeerClient>,
    controllers: std::collections::HashMap<String, String>,
}

async fn fetch_controlled_tabs(source: &PeerSource) -> Option<Vec<serde_json::Value>> {
    let command = crate::mesh::types::RelayCommand {
        method: "GET".into(),
        path: "/api/tabs?local_only=true".into(),
        body: serde_json::Value::Null,
        delivery_id: None,
    };
    let response = source.client.as_ref()?.send_relay(&command).await.ok()?;
    if !response.ok {
        return None;
    }
    let serde_json::Value::Array(tabs) = response.data? else {
        return None;
    };
    Some(
        tabs.into_iter()
            .filter(|tab| {
                tab.get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| source.controllers.contains_key(id))
            })
            .collect(),
    )
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
    if registered.is_empty() && members.is_empty() && cache().lock().unwrap().is_empty() {
        return Ok(scope_ordinary_tab_projection(local, workspace_id));
    }
    let mesh = mesh.clone();
    // Routing and outbound request signing stay off runtime workers and never sign under the mesh lock.
    let (paired_ids, sources, members) = tokio::task::spawn_blocking(move || {
        let (paired_ids, signing, raw_sources) = {
            let manager = mesh.lock();
            let paired_ids = manager
                .paired_peers
                .keys()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let signing = manager.signing_credentials();
            let raw_sources = manager
                .paired_peers
                .values()
                .filter_map(|peer| {
                    let mut controllers = registered
                        .iter()
                        .flat_map(|(controller, workers)| {
                            workers
                                .iter()
                                .filter(|worker| {
                                    worker.peer == peer.instance_id
                                        || worker.peer == peer.instance_name
                                })
                                .map(|worker| (worker.remote_tab.clone(), controller.clone()))
                        })
                        .collect::<std::collections::HashMap<_, _>>();
                    for member in members
                        .iter()
                        .filter(|member| member.peer_id == peer.instance_id)
                    {
                        if member.finalized {
                            controllers.remove(&member.tab_id);
                        } else {
                            controllers
                                .insert(member.tab_id.clone(), member.orchestrator_tab_id.clone());
                        }
                    }
                    (!controllers.is_empty()).then(|| {
                        (
                            peer.instance_id.clone(),
                            peer.instance_name.clone(),
                            peer.online,
                            peer.online
                                .then(|| manager.peer_route_endpoints(&peer.instance_id))
                                .flatten(),
                            controllers,
                        )
                    })
                })
                .collect::<Vec<_>>();
            (paired_ids, signing, raw_sources)
        };
        let sources = raw_sources
            .into_iter()
            .map(
                |(peer_id, peer_name, online, endpoints, controllers)| PeerSource {
                    peer_id,
                    peer_name,
                    online,
                    client: endpoints.and_then(|(primary, alternatives)| {
                        let (instance_id, private_key) = signing.as_ref()?;
                        let token =
                            crate::mesh::crown::sign_request(private_key, instance_id).ok()?;
                        Some(crate::mesh::transport::PeerClient::new_signed_route(
                            primary,
                            alternatives,
                            &token,
                        ))
                    }),
                    controllers,
                },
            )
            .collect::<Vec<_>>();
        (paired_ids, sources, members)
    })
    .await
    .map_err(|error| error.to_string())?;
    cache()
        .lock()
        .unwrap()
        .retain(|peer, _| paired_ids.contains(peer));
    let fetched_tabs =
        futures_util::future::join_all(sources.iter().map(fetch_controlled_tabs)).await;
    let mut remote = Vec::new();
    for (source, fetched) in sources.into_iter().zip(fetched_tabs) {
        let PeerSource {
            peer_id,
            peer_name,
            online,
            controllers,
            ..
        } = source;
        let peer_members = members
            .iter()
            .filter(|member| member.peer_id == peer_id)
            .cloned()
            .collect::<Vec<_>>();
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
    if remote.is_empty() {
        return Ok(scope_ordinary_tab_projection(local, workspace_id));
    }
    let workspace_id = workspace_id.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        scope_ordinary_tab_projection(
            merge_ordinary_tab_projection(&local, &remote),
            workspace_id.as_deref(),
        )
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
