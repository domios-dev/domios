use serde::{Deserialize, Serialize};

use super::remote_worker_transfer::{RemoteWorkerTransfer, RemoteWorkerTransferReceipt};

mod commands;
mod origin;
mod store;
mod worker;
pub use origin::{RemoteTeamPeer, RemoteTransferTarget};
pub use store::{RemoteTeamData, RemoteTeamStore};
pub use worker::{
    RemoteFinalizationReceipt, RemoteWorkerFinalize, RemoteWorkerInfo, RemoteWorkerLookup,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTeamMember {
    pub peer_id: String,
    pub tab_id: String,
    pub session_id: String,
    pub title: String,
    pub provider: String,
    pub orchestrator_tab_id: String,
    pub team_id: String,
    pub operation_id: String,
    #[serde(default)]
    pub finalized: bool,
}

pub fn member_key(peer: &str, tab: &str) -> String {
    format!("{peer}/{tab}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingTransfer {
    pub operation_id: String,
    pub peer_id: String,
    pub request: RemoteWorkerTransfer,
    pub member: RemoteTeamMember,
    pub actor_tab_id: Option<String>,
    pub actor_session_id: Option<String>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingTransfer {
    pub origin_peer_id: String,
    pub request: RemoteWorkerTransfer,
    pub receipt: RemoteWorkerTransferReceipt,
}

#[cfg(test)]
mod tests;
