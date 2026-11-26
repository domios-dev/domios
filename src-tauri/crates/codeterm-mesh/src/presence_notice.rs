use crate::{MeshManager, MeshState};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "/api/mesh/account-presence";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, untagged)]
pub enum PresenceNotice {
    Endpoint {
        endpoint_record: crate::endpoint_gossip::SignedEndpointRecord,
    },
    Snapshot {
        signed_registry: codeterm_entitlements::DetachedGrant,
    },
    Presence {
        metadata_revision: u64,
    },
}

pub async fn broadcast_registry_snapshot(
    mesh: &MeshState,
    snapshot: &codeterm_entitlements::DetachedGrant,
) {
    if mesh.lock().inbound_only {
        return;
    }
    let targets = mesh
        .lock()
        .endpoint_records()
        .into_iter()
        .filter_map(|record| {
            let route = record.claim.endpoint.as_ref()?.cloud_endpoint()?;
            Some((record.claim.installation_id, route))
        })
        .collect::<Vec<_>>();
    let mut direct = tokio::task::JoinSet::new();
    for (installation_id, route) in targets {
        let mesh = mesh.clone();
        let registry = snapshot.clone();
        direct.spawn(async move {
            if let Ok(channel) =
                crate::cloud_ingress::connect_enrolled(mesh, route, &installation_id).await
            {
                let _ = channel.push_registry(registry).await;
            }
        });
    }
    broadcast(
        mesh,
        PresenceNotice::Snapshot {
            signed_registry: snapshot.clone(),
        },
    )
    .await;
    while direct.join_next().await.is_some() {}
}

pub async fn broadcast_presence_notice(mesh: &MeshState, metadata_revision: u64) {
    broadcast(mesh, PresenceNotice::Presence { metadata_revision }).await;
}

pub async fn broadcast_endpoint_record(
    mesh: &MeshState,
    endpoint_record: crate::endpoint_gossip::SignedEndpointRecord,
) {
    broadcast(mesh, PresenceNotice::Endpoint { endpoint_record }).await;
}

async fn broadcast(mesh: &MeshState, notice: PresenceNotice) {
    let (token, peers) = {
        let manager = mesh.lock();
        if manager.inbound_only {
            return;
        }
        let Ok(token) = manager.sign_request() else {
            return;
        };
        (token, notice_targets(&manager))
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

fn notice_targets(manager: &MeshManager) -> Vec<(Vec<crate::types::PeerAddress>, u16)> {
    manager
        .paired_peers
        .values()
        .filter(|peer| peer.online && !manager.account_peer_revoked(&peer.instance_id))
        .map(|peer| (peer.addresses.clone(), peer.api_port))
        .collect()
}

#[cfg(test)]
mod tests;
