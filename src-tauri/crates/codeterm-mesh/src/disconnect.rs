use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::account_mesh::transcript;
use crate::types::PairedPeer;
use crate::{MeshManager, MeshState};

pub const DISCONNECT_PATH: &str = "/api/mesh/disconnect-notice";
const ROLE: &[u8] = b"domios-mesh-disconnect-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisconnectNotice {
    pub from: String,
    pub to: String,
    pub link_nonce: Option<String>,
    pub signature: String,
}

impl DisconnectNotice {
    fn digest(&self) -> [u8; 32] {
        Sha256::digest(
            serde_json::to_vec(&(&self.from, &self.to, &self.link_nonce)).unwrap_or_default(),
        )
        .into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDisconnect {
    pub public_key: String,
    pub notice: DisconnectNotice,
}

impl MeshManager {
    pub(crate) fn make_disconnect(&self, peer: &PairedPeer) -> Result<SavedDisconnect, String> {
        let mut notice = DisconnectNotice {
            from: self.ident.identity.instance_id.clone(),
            to: peer.instance_id.clone(),
            link_nonce: self
                .link_record(&peer.instance_id)
                .and_then(|record| record.link_nonce.clone()),
            signature: String::new(),
        };
        let key = self
            .ident
            .identity
            .private_key
            .as_deref()
            .ok_or("Mesh identity has no signing key")?;
        notice.signature = transcript::sign_mesh(key, &notice.digest(), ROLE)?;
        Ok(SavedDisconnect {
            public_key: peer.public_key.clone(),
            notice,
        })
    }

    pub fn disconnect_notice_for(&self, token: &str) -> Option<DisconnectNotice> {
        let keys = self.disconnect_keys();
        let sender = crate::crown::verify_signed_request(token, &keys).ok()?;
        self.saved_disconnect(&sender)
            .map(|saved| saved.notice.clone())
    }

    pub fn apply_disconnect_notice(&mut self, notice: &DisconnectNotice) -> Result<bool, String> {
        if notice.to != self.ident.identity.instance_id {
            return Err("Disconnect names another recipient".into());
        }
        let Some(peer) = self.paired_peers.get(&notice.from) else {
            return Ok(false);
        };
        if self
            .link_record(&notice.from)
            .and_then(|record| record.link_nonce.as_ref())
            != notice.link_nonce.as_ref()
            || !transcript::verify_mesh(&peer.public_key, &notice.digest(), ROLE, &notice.signature)
        {
            return Err("Disconnect proof does not match this connection".into());
        }
        self.unpair(&notice.from).map(|removed| removed.is_some())
    }
}

pub async fn deliver_disconnect(peer: PairedPeer, notice: DisconnectNotice, bearer: String) {
    let _ = crate::signed_notice::send(
        &peer.addresses,
        peer.api_port,
        DISCONNECT_PATH,
        &bearer,
        &notice,
    )
    .await;
}

pub async fn ping_peer(
    mesh: &MeshState,
    peer_id: &str,
    client: &crate::transport::PeerClient,
) -> bool {
    let Ok(reply) = client.ping_reply().await else {
        return false;
    };
    if let Some(value) = reply.get("disconnect_notice") {
        if let Ok(notice) = serde_json::from_value::<DisconnectNotice>(value.clone()) {
            if notice.from == peer_id {
                let _ = mesh.lock().apply_disconnect_notice(&notice);
            }
        }
    }
    true
}
