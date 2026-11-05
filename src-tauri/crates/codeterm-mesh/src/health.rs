use std::future::Future;

use crate::transport::{PeerClient, SyncFailure, TryNext};
use crate::types::{MeshSyncPayload, PeerAddress};
use crate::{MeshManager, MeshState};

pub(crate) struct PeerEndpoint {
    pub addresses: Vec<PeerAddress>,
    pub port: u16,
    pub token: String,
}

struct HealthProbe {
    endpoint: PeerEndpoint,
    payload: MeshSyncPayload,
    account_id: String,
    account_epoch: u64,
    link_nonce: Option<String>,
    online: bool,
    now: u64,
}

impl HealthProbe {
    fn capture(manager: &mut MeshManager, peer_id: &str) -> Option<Self> {
        if !manager.traffic_open(peer_id) || !manager.peer_requests_allowed(peer_id) {
            return None;
        }
        let peer = manager.paired_peers.get(peer_id)?;
        let endpoint = PeerEndpoint {
            addresses: peer.addresses.clone(),
            port: peer.api_port,
            token: manager.sign_request_for(peer_id).ok()?,
        };
        let online = peer.online;
        let account_id = manager.account_state().signed_in_account()?.to_owned();
        let account_epoch = manager.account_state().account_epoch;
        let link_nonce = manager.link_record(peer_id)?.link_nonce.clone();
        Some(Self {
            endpoint,
            payload: manager.build_sync_payload(),
            account_id,
            account_epoch,
            link_nonce,
            online,
            now: manager.now(),
        })
    }
}

pub async fn check_peer(mesh: &MeshState, peer_id: &str, last_sync: Option<u64>) -> Option<bool> {
    check_peer_with(
        mesh,
        peer_id,
        last_sync,
        ping,
        |client, payload| async move { client.send_sync(&payload).await },
    )
    .await
}

async fn ping(endpoint: PeerEndpoint) -> Result<PeerClient, ()> {
    crate::transport::try_peer(&endpoint.addresses, |address| {
        let client = PeerClient::new_signed(&address.ip, endpoint.port, &endpoint.token);
        async move {
            if client.ping_reply().await.is_ok() {
                TryNext::Success(client)
            } else {
                TryNext::Retry("ping failed".into())
            }
        }
    })
    .await
    .map_err(|_| ())
}

pub(crate) async fn check_peer_with<C, P, PF, S, SF>(
    mesh: &MeshState,
    peer_id: &str,
    last_sync: Option<u64>,
    ping: P,
    sync: S,
) -> Option<bool>
where
    P: FnOnce(PeerEndpoint) -> PF,
    PF: Future<Output = Result<C, ()>>,
    S: FnOnce(C, MeshSyncPayload) -> SF,
    SF: Future<Output = Result<MeshSyncPayload, SyncFailure>>,
{
    let probe = HealthProbe::capture(&mut mesh.lock(), peer_id)?;
    let result = match ping(probe.endpoint).await {
        Ok(client) => {
            if !crate::types::should_proactive_sync(
                true,
                probe.online,
                probe.now,
                last_sync,
                crate::discovery::PROACTIVE_SYNC_INTERVAL_SECS,
            ) {
                return None;
            }
            sync(client, probe.payload).await
        }
        Err(()) => Err(SyncFailure::Request),
    };
    let status = {
        let mut manager = mesh.lock();
        if manager.account_state().account_epoch != probe.account_epoch
            || manager.account_state().signed_in_account() != Some(probe.account_id.as_str())
            || manager
                .link_record(peer_id)
                .is_none_or(|record| record.link_nonce != probe.link_nonce)
            || !manager.traffic_open(peer_id)
        {
            return None;
        }
        let status = match &result {
            Ok(_) => 200,
            Err(SyncFailure::Refused(status)) => *status,
            Err(_) => 0,
        };
        manager.record_peer_http_status(peer_id, probe.link_nonce.as_deref(), status);
        status
    };
    if let Ok(payload) = result {
        crate::endpoint_gossip::receive_endpoint_records(mesh, payload.endpoints).await;
    }
    Some((200..=299).contains(&status))
}
