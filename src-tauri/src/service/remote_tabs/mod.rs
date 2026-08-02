use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum OrdinaryTabKey {
    Local { tab_id: String },
    Remote { peer_id: String, tab_id: String },
}

impl OrdinaryTabKey {
    pub fn local(tab_id: impl Into<String>) -> Self {
        Self::Local { tab_id: tab_id.into() }
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
    pub tabs: Vec<serde_json::Value>,
}

pub fn merge_ordinary_tab_projection(
    local: &[serde_json::Value],
    peers: &[RemotePeerProjection],
) -> Vec<serde_json::Value> {
    let mut projected = local.to_vec();
    for peer in peers.iter().filter(|peer| peer.authenticated && peer.reachable) {
        for source in &peer.tabs {
            let Some(tab_id) = source.get("id").and_then(serde_json::Value::as_str) else { continue };
            let mut tab = source.clone();
            let Some(object) = tab.as_object_mut() else { continue };
            let ordinary_id = OrdinaryTabKey::remote(&peer.peer_id, tab_id).ordinary_id();
            object.insert("id".into(), serde_json::json!(ordinary_id));
            object.insert("remote_tab_id".into(), serde_json::json!(tab_id));
            object.insert("remote_peer_id".into(), serde_json::json!(peer.peer_id));
            object.insert("remote_peer_name".into(), serde_json::json!(peer.peer_name));
            projected.push(tab);
        }
    }
    projected
}

#[cfg(test)]
mod tests;
