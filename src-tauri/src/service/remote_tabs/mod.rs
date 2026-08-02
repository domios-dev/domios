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

#[cfg(test)]
mod tests;
