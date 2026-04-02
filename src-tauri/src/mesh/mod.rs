//! CodeTerm Mesh — peer-to-peer networking between CodeTerm instances.
//!
//! Provides discovery (mDNS + Tailscale), secure pairing (X25519 + AES-256-GCM),
//! and encrypted communication between paired instances.

pub mod types;
pub mod peers;
pub mod discovery;
pub mod pairing;
pub mod transport;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use types::{DiscoveredPeer, MeshIdentity, MeshPeersResponse, MeshStatus, PairedPeer, PairingRequest, RevocationList};

pub type MeshState = Arc<Mutex<MeshManager>>;

/// Central mesh state — holds identity, peers, discovery results, and pending requests.
pub struct MeshManager {
    pub identity: MeshIdentity,
    pub instance_name: String,
    pub discovered_peers: HashMap<String, DiscoveredPeer>,
    pub paired_peers: HashMap<String, PairedPeer>,
    pub pending_requests: HashMap<String, PairingRequest>,
    pub revocation_list: RevocationList,
    pub discovery_active: bool,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    /// Ephemeral X25519 keypairs for in-flight pairing requests (request_id → private key bytes).
    pub pairing_keypairs: HashMap<String, [u8; 32]>,
    /// Target peer address for outgoing pairing requests (request_id → (address, port, instance_id)).
    pub pairing_targets: HashMap<String, (String, u16, String)>,
}

impl MeshManager {
    pub fn new(instance_name: String, api_port: u16) -> Self {
        let identity = peers::load_or_create_identity();
        let paired_peers = peers::load_peers();
        let revocation_list = peers::load_revocation_list();

        log::info!(
            "[mesh] Initialized: id={}, name={}, {} paired peers",
            identity.instance_id,
            instance_name,
            paired_peers.len()
        );

        Self {
            identity,
            instance_name,
            discovered_peers: HashMap::new(),
            paired_peers,
            pending_requests: HashMap::new(),
            revocation_list,
            discovery_active: false,
            tailscale_ip: None,
            local_ip: None,
            api_port,
            pairing_keypairs: HashMap::new(),
            pairing_targets: HashMap::new(),
        }
    }

    /// Persist paired peers to disk.
    pub fn save(&self) -> Result<(), String> {
        peers::save_peers(&self.paired_peers)?;
        peers::save_revocation_list(&self.revocation_list)?;
        Ok(())
    }

    /// Get mesh status summary.
    pub fn status(&self) -> MeshStatus {
        MeshStatus {
            enabled: true,
            instance_id: self.identity.instance_id.clone(),
            instance_name: self.instance_name.clone(),
            discovery_active: self.discovery_active,
            tailscale_ip: self.tailscale_ip.clone(),
            local_ip: self.local_ip.clone(),
            api_port: self.api_port,
            paired_count: self.paired_peers.len(),
            discovered_count: self.discovered_peers.len(),
        }
    }

    /// Get all peers for the UI.
    pub fn peers_response(&self) -> MeshPeersResponse {
        MeshPeersResponse {
            paired: self.paired_peers.values().cloned().collect(),
            discovered: self
                .discovered_peers
                .values()
                // Don't show already-paired peers in discovered list
                .filter(|d| !self.paired_peers.contains_key(&d.instance_id))
                .cloned()
                .collect(),
            incoming_requests: self
                .pending_requests
                .values()
                .filter(|r| r.status == types::PairingStatus::Pending)
                .cloned()
                .collect(),
        }
    }

    /// Check if an instance_id is blocked.
    pub fn is_blocked(&self, instance_id: &str) -> bool {
        self.revocation_list.blocked.contains(&instance_id.to_string())
    }

    /// Check if an instance_id has been revoked.
    pub fn is_revoked(&self, instance_id: &str) -> bool {
        self.revocation_list.revoked.contains(&instance_id.to_string())
    }

    /// Count pending pairing requests.
    pub fn pending_request_count(&self) -> usize {
        self.pending_requests
            .values()
            .filter(|r| r.status == types::PairingStatus::Pending)
            .count()
    }

    /// Remove a paired peer and revoke its token.
    pub fn unpair(&mut self, instance_id: &str) -> Option<PairedPeer> {
        let removed = self.paired_peers.remove(instance_id);
        if removed.is_some() {
            if !self.revocation_list.revoked.contains(&instance_id.to_string()) {
                self.revocation_list.revoked.push(instance_id.to_string());
            }
            let _ = self.save();
        }
        removed
    }

    /// Set a peer as primary (clears previous primary).
    pub fn set_primary(&mut self, instance_id: &str) -> Result<(), String> {
        if !self.paired_peers.contains_key(instance_id) {
            return Err("Peer not found".to_string());
        }
        for peer in self.paired_peers.values_mut() {
            peer.is_primary = peer.instance_id == instance_id;
        }
        self.save()
    }

    /// Validate and store an incoming pairing request.
    pub fn receive_pairing_request(&mut self, request: PairingRequest) -> Result<(), String> {
        // Check blocked
        if self.is_blocked(&request.from_instance_id) {
            return Err("Instance is blocked".to_string());
        }

        // Check timestamp (60s window)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(request.timestamp) > 60 {
            return Err("Request expired".to_string());
        }

        // Rate limit: max 5 pending
        if self.pending_request_count() >= 5 {
            return Err("Too many pending requests".to_string());
        }

        // Check duplicate request_id
        if self.pending_requests.contains_key(&request.request_id) {
            return Err("Duplicate request ID".to_string());
        }

        log::info!(
            "[mesh] Incoming pairing request from {} ({})",
            request.from_instance_name,
            request.from_instance_id
        );

        self.pending_requests
            .insert(request.request_id.clone(), request);
        Ok(())
    }

    /// Clean up expired pairing requests (>60s old).
    pub fn cleanup_expired_requests(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.pending_requests.retain(|_, r| {
            now.saturating_sub(r.timestamp) <= 120 // 2 min grace for network delays
        });
    }

    /// Authenticate a mesh token — returns the peer's instance_id if valid.
    pub fn authenticate_mesh_token(&self, token: &str) -> Option<String> {
        for (id, peer) in &self.paired_peers {
            if peer.mesh_token == token {
                if self.is_revoked(id) {
                    return None;
                }
                return Some(id.clone());
            }
        }
        None
    }
}
