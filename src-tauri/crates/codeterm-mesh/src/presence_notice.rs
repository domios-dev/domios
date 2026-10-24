use crate::{transport, MeshManager, MeshState};
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
        let http = transport::shared_http_client();
        let token = token.clone();
        sends.spawn(async move {
            let notice = PresenceNotice { metadata_revision };
            let _ = transport::try_peer(&addresses, |address| {
                let request = http
                    .post(format!(
                        "{}{PATH}",
                        transport::peer_http_origin(&address.ip, port)
                    ))
                    .header("Authorization", format!("Bearer {token}"))
                    .header(
                        crate::version::VERSION_HEADER,
                        crate::version::local_version(),
                    )
                    .json(&notice);
                async move {
                    match request.send().await {
                        Ok(response) if response.status().is_success() => {
                            transport::TryNext::Success(())
                        }
                        Ok(_) => transport::TryNext::Stop("presence notice refused".into()),
                        Err(_) => transport::TryNext::Retry("presence notice unavailable".into()),
                    }
                }
            })
            .await;
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
