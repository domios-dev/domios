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
    /// The mesh-wide crown token that all peers agree on.
    /// Only the holder of this token is the legitimate crown holder.
    pub mesh_crown_token: Option<String>,
    /// Instance ID of the current crown holder (as known by this peer).
    pub crown_holder_id: Option<String>,
}

impl MeshManager {
    pub fn new(instance_name: String, api_port: u16) -> Self {
        let identity = peers::load_or_create_identity();
        let paired_peers = peers::load_peers();
        let revocation_list = peers::load_revocation_list();

        log::info!(
            "[mesh] Initialized: id={}, name={}, {} paired peers, crown={}",
            identity.instance_id,
            instance_name,
            paired_peers.len(),
            identity.crown_token.is_some()
        );

        let crown_token = identity.crown_token.clone();
        let holder_id = crown_token.as_ref().map(|_| identity.instance_id.clone());

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
            mesh_crown_token: crown_token,
            crown_holder_id: holder_id,
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
            is_crown: self.identity.crown_token.is_some(),
            crown_holder: self.crown_holder_id.clone(),
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
                // Only show requests FROM others, not our own outgoing ones
                .filter(|r| {
                    r.status == types::PairingStatus::Pending
                        && r.from_instance_id != self.identity.instance_id
                })
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

    /// Transfer the crown to a paired peer. Only works if we hold the crown_token.
    pub fn transfer_crown(&mut self, instance_id: &str) -> Result<String, String> {
        if self.identity.crown_token.is_none() {
            return Err("Only the crown holder can transfer the crown".to_string());
        }
        if !self.paired_peers.contains_key(instance_id) {
            return Err("Peer not found".to_string());
        }
        // Generate new crown token for the transfer
        let new_token = uuid::Uuid::new_v4().to_string();
        // Relinquish our crown
        self.identity.crown_token = None;
        peers::save_identity(&self.identity);
        // Update mesh-wide tracking
        self.mesh_crown_token = Some(new_token.clone());
        self.crown_holder_id = Some(instance_id.to_string());
        log::info!("[mesh] Crown transferred to {}", instance_id);
        let _ = self.save();
        Ok(new_token)
    }

    /// Receive the crown from a transfer.
    pub fn receive_crown(&mut self, crown_token: &str, from_id: &str) {
        // Verify the sender is a known peer (can't receive from strangers)
        if !self.paired_peers.contains_key(from_id) {
            log::warn!("[mesh] Crown transfer from unknown peer {}", from_id);
            return;
        }
        self.identity.crown_token = Some(crown_token.to_string());
        self.mesh_crown_token = Some(crown_token.to_string());
        self.crown_holder_id = Some(self.identity.instance_id.clone());
        peers::save_identity(&self.identity);
        log::info!("[mesh] Received crown from {} — this instance is now primary", from_id);
    }

    /// Claim crown if nobody has it (first instance to enable mesh).
    pub fn claim_crown_if_unclaimed(&mut self) {
        if self.identity.crown_token.is_some() {
            return;
        }
        // If we have no paired peers, we're the first — claim it
        if self.paired_peers.is_empty() {
            let token = uuid::Uuid::new_v4().to_string();
            self.identity.crown_token = Some(token.clone());
            self.mesh_crown_token = Some(token);
            self.crown_holder_id = Some(self.identity.instance_id.clone());
            peers::save_identity(&self.identity);
            log::info!("[mesh] Claimed crown — first instance in mesh");
        }
    }

    /// Update crown info received from a peer (e.g. during gossip sync).
    pub fn update_crown_info(&mut self, crown_token: &str, holder_id: &str) {
        self.mesh_crown_token = Some(crown_token.to_string());
        self.crown_holder_id = Some(holder_id.to_string());
    }

    /// Validate and store an incoming pairing request.
    pub fn receive_pairing_request(&mut self, request: PairingRequest) -> Result<(), String> {
        // Ignore our own requests
        if request.from_instance_id == self.identity.instance_id {
            return Err("Own request".to_string());
        }

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
