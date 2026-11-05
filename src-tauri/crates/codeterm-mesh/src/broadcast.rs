use std::sync::Arc;
use std::time::Duration;

use crate::account_mesh::BoxFuture;
use crate::types::{PeerAddress, PeerAnnounce};
use crate::{transport, MeshState};

pub(crate) mod coalesce;
pub(crate) mod refresh;

const MAX_CONCURRENT_ANNOUNCES: usize = 8;
const PEER_ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AnnounceReport {
    pub sent: usize,
    pub ok: usize,
    pub stale_peers: Vec<String>,
    pub errors: Vec<String>,
}

fn peer_request_error(peer_id: &str, _detail: &str) -> String {
    format!("{peer_id}: request failed")
}

struct AnnounceWork {
    addresses: Vec<PeerAddress>,
    port: u16,
    token: String,
    peer_id: String,
    announce: PeerAnnounce,
    membership: Option<String>,
}

enum SendOutcome {
    Acknowledged,
    Stale,
    Failed,
}

type SendPort = dyn Fn(AnnounceWork) -> BoxFuture<'static, SendOutcome> + Send + Sync;

pub async fn broadcast_announce(mesh: &MeshState, target: Option<&str>) -> AnnounceReport {
    broadcast_with(
        mesh,
        target,
        None,
        Arc::new(|work| Box::pin(send(work))),
        PEER_ANNOUNCE_TIMEOUT,
    )
    .await
}

pub async fn broadcast_address_change(mesh: &MeshState) -> AnnounceReport {
    coalesce::publish_with(mesh, |mesh, snapshot| {
        Box::pin(broadcast_with(
            mesh,
            None,
            Some(snapshot),
            Arc::new(|work| Box::pin(send(work))),
            PEER_ANNOUNCE_TIMEOUT,
        ))
    })
    .await
}

async fn broadcast_with(
    mesh: &MeshState,
    target: Option<&str>,
    snapshot: Option<PeerAnnounce>,
    sender: Arc<SendPort>,
    deadline: Duration,
) -> AnnounceReport {
    let work = {
        let manager = mesh.lock();
        let token = match manager.sign_request() {
            Ok(token) => token,
            Err(error) => {
                return AnnounceReport {
                    errors: vec![format!("sign: {error}")],
                    ..Default::default()
                }
            }
        };
        let announce = snapshot.unwrap_or_else(|| manager.build_announce());
        let membership = manager.advertised_membership();
        let mut work: Vec<_> = manager
            .paired_peers
            .values()
            .filter(|peer| {
                target
                    .is_none_or(|target| target == peer.instance_id || target == peer.instance_name)
            })
            .filter(|peer| {
                manager.announce_allowed(&peer.instance_id) && !peer.addresses.is_empty()
            })
            .map(|peer| AnnounceWork {
                addresses: peer.addresses.clone(),
                port: peer.api_port,
                token: token.clone(),
                peer_id: peer.instance_id.clone(),
                announce: announce.clone(),
                membership: membership.clone(),
            })
            .collect();
        work.sort_by(|left, right| left.peer_id.cmp(&right.peer_id));
        work
    };
    fanout(mesh, work, sender, deadline).await
}

async fn fanout(
    mesh: &MeshState,
    work: Vec<AnnounceWork>,
    sender: Arc<SendPort>,
    deadline: Duration,
) -> AnnounceReport {
    let slots = mesh.lock().announce_slots.clone();
    let mut report = AnnounceReport::default();
    let mut remaining = work.into_iter();
    let mut sends = tokio::task::JoinSet::new();
    loop {
        while sends.len() < MAX_CONCURRENT_ANNOUNCES {
            let Some(work) = remaining.next() else { break };
            let sender = sender.clone();
            let slots = slots.clone();
            report.sent += 1;
            sends.spawn(async move {
                let peer_id = work.peer_id.clone();
                let outcome = tokio::time::timeout(deadline, async move {
                    let Ok(_slot) = slots.acquire_owned().await else {
                        return SendOutcome::Failed;
                    };
                    sender(work).await
                })
                .await
                .unwrap_or(SendOutcome::Failed);
                (peer_id, outcome)
            });
        }
        let Some(result) = sends.join_next().await else {
            break;
        };
        match result {
            Ok((peer_id, SendOutcome::Acknowledged)) => {
                report.ok += 1;
                mesh.lock().record_announce_outcome(&peer_id, true);
            }
            Ok((peer_id, SendOutcome::Stale)) => report.stale_peers.push(peer_id),
            Ok((peer_id, SendOutcome::Failed)) => {
                mesh.lock().record_announce_outcome(&peer_id, false);
                report
                    .errors
                    .push(peer_request_error(&peer_id, "request failed"));
            }
            Err(_) => report.errors.push("announce worker failed".into()),
        }
    }
    report.stale_peers.sort();
    report.errors.sort();
    report
}

async fn send(work: AnnounceWork) -> SendOutcome {
    let http = transport::shared_http_client();
    transport::try_peer(&work.addresses, |address| {
        let url = format!(
            "{}/api/mesh/announce",
            transport::peer_http_origin(&address.ip, work.port)
        );
        let http = http.clone();
        let token = work.token.clone();
        let announce = work.announce.clone();
        let membership = work.membership.clone();
        async move {
            let mut request = http
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .header(
                    crate::version::VERSION_HEADER,
                    crate::version::local_version(),
                );
            if let Some(membership) = membership {
                request =
                    request.header(crate::account_mesh::protocol::MEMBERSHIP_HEADER, membership);
            }
            match request.json(&announce).send().await {
                Ok(response) if response.status().is_success() => {
                    transport::TryNext::Success(SendOutcome::Acknowledged)
                }
                Ok(response) if response.status().as_u16() == 404 => {
                    transport::TryNext::Success(SendOutcome::Stale)
                }
                Ok(_) => transport::TryNext::Stop("peer refused announce".into()),
                Err(_) => transport::TryNext::Retry("request failed".into()),
            }
        }
    })
    .await
    .unwrap_or(SendOutcome::Failed)
}

pub async fn refresh_local_addresses(mesh: &MeshState) -> bool {
    let mesh = mesh.clone();
    tokio::task::spawn_blocking(move || refresh::refresh(&mesh, false))
        .await
        .unwrap_or_else(|error| {
            log::error!("[mesh/refresh] address worker failed: {error}");
            false
        })
}

pub fn refresh_local_addresses_blocking(mesh: &MeshState) -> bool {
    refresh::refresh(mesh, false)
}

pub fn refresh_local_addresses_blocking_force(mesh: &MeshState) -> bool {
    refresh::refresh(mesh, true)
}

#[cfg(test)]
mod tests;
