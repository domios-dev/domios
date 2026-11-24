use std::time::Duration;

use axum::{
    body::Body,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use codeterm_service_core::{
    mesh::{manager::MeshManager, types::AccessLevel},
    MeshStateHandle,
};
use futures_util::StreamExt;

use crate::routes::AppState;

#[derive(Clone, Debug)]
pub(crate) struct PeerGrant {
    peer_id: String,
    access: AccessLevel,
    grant_epoch: u64,
    account_epoch: u64,
    link_nonce: Option<String>,
    public_key: String,
}

impl PeerGrant {
    pub(crate) fn capture(mesh: &MeshManager, peer_id: &str) -> Option<Self> {
        let peer = mesh.paired_peers.get(peer_id)?;
        let link = mesh.link_record(peer_id)?;
        mesh.traffic_open(peer_id).then(|| Self {
            peer_id: peer_id.to_owned(),
            access: peer.access_level.clone(),
            grant_epoch: mesh.peer_grant_epoch(peer_id),
            account_epoch: mesh.account_state().account_epoch,
            link_nonce: link.link_nonce.clone(),
            public_key: peer.public_key.clone(),
        })
    }

    pub(crate) fn access(&self) -> &AccessLevel {
        &self.access
    }

    pub(crate) fn current(&self, mesh: &MeshManager) -> bool {
        mesh.traffic_open(&self.peer_id)
            && mesh.peer_grant_epoch(&self.peer_id) == self.grant_epoch
            && mesh.account_state().account_epoch == self.account_epoch
            && mesh
                .link_record(&self.peer_id)
                .is_some_and(|link| link.link_nonce == self.link_nonce)
            && mesh.paired_peers.get(&self.peer_id).is_some_and(|peer| {
                peer.access_level == self.access && peer.public_key == self.public_key
            })
    }

    pub(crate) async fn changed(&self, mesh: &MeshStateHandle) {
        loop {
            let (mut watch, deadline) = {
                let manager = mesh.lock();
                if !self.current(&manager) {
                    return;
                }
                (
                    manager.gate_watch(),
                    manager.admission_deadline(&self.peer_id).map(|deadline| {
                        Duration::from_secs(deadline.saturating_sub(manager.now()))
                    }),
                )
            };
            if let Some(deadline) = deadline {
                tokio::select! {
                    result = watch.changed() => if result.is_err() { return },
                    _ = tokio::time::sleep(deadline) => {}
                }
            } else if watch.changed().await.is_err() {
                return;
            }
        }
    }

    pub(crate) fn refusal() -> Response {
        let code = codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED;
        (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": {"code":code,"message":"Peer authorization changed"}
            })),
        )
            .into_response()
    }

    pub(crate) fn fence_response(
        self,
        response: Response,
        state: AppState,
        method: Method,
        uri: String,
    ) -> Response {
        if !self.current(&state.mesh.lock()) {
            return Self::refusal();
        }
        let (parts, body) = response.into_parts();
        let stream = async_stream::stream! {
            let mesh = state.mesh.clone();
            let mut changed = std::pin::pin!(self.changed(&mesh));
            let mut bytes = body.into_data_stream();
            loop {
                let item = tokio::select! {
                    biased;
                    _ = &mut changed => break,
                    item = bytes.next() => item,
                };
                let Some(item) = item else { break };
                if self.access == AccessLevel::AgentIsolated
                    && !super::authorize_peer_request(&state, &self.access, &self.peer_id, &method, &uri)
                {
                    break;
                }
                if !self.current(&mesh.lock()) {
                    break;
                }
                yield item;
            }
        };
        Response::from_parts(parts, Body::from_stream(stream))
    }
}
