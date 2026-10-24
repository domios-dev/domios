use crate::{MeshManager, MeshState};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "/api/mesh/account-presence";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceNotice {
    pub metadata_revision: u64,
}

pub async fn broadcast_presence_notice(mesh: &MeshState, metadata_revision: u64) {
    let (token, peers) = {
        let manager = mesh.lock();
        let Ok(token) = manager.sign_request() else {
            return;
        };
        let peers = notice_targets(&manager);
        (token, peers)
    };
    let mut sends = tokio::task::JoinSet::new();
    for (addresses, port) in peers {
        let token = token.clone();
        sends.spawn(async move {
            let notice = PresenceNotice { metadata_revision };
            let _ = crate::signed_notice::send(&addresses, port, PATH, &token, &notice).await;
        });
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while sends.join_next().await.is_some() {}
    })
    .await;
}

fn notice_targets(manager: &MeshManager) -> Vec<(Vec<crate::types::PeerAddress>, u16)> {
    manager
        .paired_peers
        .values()
        .filter(|peer| {
            peer.online
                && !manager.is_revoked(&peer.instance_id)
                && !manager.is_blocked(&peer.instance_id)
        })
        .map(|peer| (peer.addresses.clone(), peer.api_port))
        .collect()
}

#[cfg(test)]
mod tests;
