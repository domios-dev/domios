use crate::{MeshManager, MeshState};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "/api/mesh/account-presence";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, untagged)]
pub enum PresenceNotice {
    Presence {
        metadata_revision: u64,
    },
    Registry {
        account_id: String,
        registry_revision: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        security_digest: Option<String>,
    },
}

pub async fn broadcast_presence_notice(mesh: &MeshState, metadata_revision: u64) {
    broadcast(mesh, PresenceNotice::Presence { metadata_revision }, false).await;
}

pub async fn broadcast_registry_notice(mesh: &MeshState) {
    let account_id = mesh
        .lock()
        .account_state()
        .signed_in_account()
        .map(str::to_owned);
    if let Some(account_id) = account_id {
        broadcast(
            mesh,
            PresenceNotice::Registry {
                account_id,
                registry_revision: None,
                security_digest: None,
            },
            true,
        )
        .await;
    }
}

async fn broadcast(mesh: &MeshState, notice: PresenceNotice, include_offline: bool) {
    let (token, peers) = {
        let manager = mesh.lock();
        if let PresenceNotice::Registry { account_id, .. } = &notice {
            if manager.account_state().signed_in_account() != Some(account_id.as_str()) {
                return;
            }
        }
        let Ok(token) = manager.sign_request() else {
            return;
        };
        (token, notice_targets(&manager, include_offline))
    };
    let mut sends = tokio::task::JoinSet::new();
    for (addresses, port) in peers {
        let token = token.clone();
        let notice = notice.clone();
        sends.spawn(async move {
            let _ = crate::signed_notice::send(&addresses, port, PATH, &token, &notice).await;
        });
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while sends.join_next().await.is_some() {}
    })
    .await;
}

fn notice_targets(
    manager: &MeshManager,
    include_offline: bool,
) -> Vec<(Vec<crate::types::PeerAddress>, u16)> {
    manager
        .paired_peers
        .values()
        .filter(|peer| {
            (include_offline || peer.online)
                && !manager.is_revoked(&peer.instance_id)
                && !manager.is_blocked(&peer.instance_id)
        })
        .map(|peer| (peer.addresses.clone(), peer.api_port))
        .collect()
}

#[cfg(test)]
mod tests;
