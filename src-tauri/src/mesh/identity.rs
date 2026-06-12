//! Identity management — signing, authentication, keypair operations.

use super::crown;
use super::types::{MeshIdentity, PairedPeer};
use std::collections::HashMap;

/// Manages this instance's identity and request signing/verification.
pub struct IdentityManager {
    pub identity: MeshIdentity,
    pub instance_name: String,
}

impl IdentityManager {
    pub fn new(identity: MeshIdentity, instance_name: String) -> Self {
        Self {
            identity,
            instance_name,
        }
    }

    /// Sign a mesh request with this instance's Ed25519 private key.
    pub fn sign_request(&self) -> Result<String, String> {
        let priv_key = self
            .identity
            .private_key
            .as_ref()
            .ok_or("No private key in identity")?;
        crown::sign_request(priv_key, &self.identity.instance_id)
    }

    /// Build map of instance_id → public_key for all paired peers.
    pub fn known_public_keys(
        &self,
        paired_peers: &HashMap<String, PairedPeer>,
    ) -> HashMap<String, String> {
        paired_peers
            .iter()
            .map(|(id, peer)| (id.clone(), peer.public_key.clone()))
            .collect()
    }

    /// Authenticate a signed mesh request. Returns sender's instance_id if valid.
    pub fn authenticate_request(
        &self,
        signed_token: &str,
        paired_peers: &HashMap<String, PairedPeer>,
    ) -> Option<String> {
        let known_keys = self.known_public_keys(paired_peers);
        match crown::verify_signed_request(signed_token, &known_keys) {
            Ok(peer_id) => {
                log::debug!("[mesh/auth] OK — peer {} verified", peer_id);
                Some(peer_id)
            }
            Err(e) => {
                log::debug!("[mesh/auth] FAILED: {}", e);
                None
            }
        }
    }
}
