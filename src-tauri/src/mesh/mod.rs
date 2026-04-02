//! CodeTerm Mesh — peer-to-peer networking between CodeTerm instances.
//!
//! Provides discovery (mDNS + Tailscale), secure pairing (X25519 + AES-256-GCM),
//! and encrypted communication between paired instances.

pub mod types;
pub mod peers;
pub mod discovery;
pub mod pairing;
pub mod transport;
pub mod crown;

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
    /// Outgoing pairing target info (request_id → (address, port, instance_id)).
    pub pairing_targets: HashMap<String, (String, u16, String)>,
    /// Crown keypair — only present if this instance is the crown holder.
    pub crown_keypair: Option<crown::CrownKeyPair>,
    /// Public crown info — known to all peers, used to verify crown commands.
    pub crown_public: Option<crown::CrownPublicInfo>,
    /// Trust declarations signed by the crown holder.
    pub trust_declarations: Vec<crown::TrustDeclaration>,
    /// Emergency election votes (pending).
    pub emergency_votes: Vec<crown::EmergencyVote>,
}

impl MeshManager {
    pub fn new(instance_name: String, api_port: u16) -> Self {
        let identity = peers::load_or_create_identity();
        let paired_peers = peers::load_peers();
        let revocation_list = peers::load_revocation_list();

        let crown_keypair = crown::load_crown_keypair();

        log::info!(
            "[mesh] Initialized: id={}, name={}, {} paired peers, crown={}",
            identity.instance_id,
            instance_name,
            paired_peers.len(),
            crown_keypair.is_some()
        );
        let crown_public = crown::load_crown_public_info();
        let trust_declarations = crown::load_trust_declarations();

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
            pairing_targets: HashMap::new(),
            crown_keypair,
            crown_public,
            trust_declarations,
            emergency_votes: Vec::new(),
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
            build: env!("BUILD_HASH").to_string(),
            is_crown: self.is_crown_holder(),
            crown_holder: self.crown_holder_id(),
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
        let paired: Vec<PairedPeer> = self.paired_peers.values()
            .map(|p| {
                let trust = self.get_trust_level(&p.instance_id);
                let mut peer = p.clone();
                peer.trust_level = match trust {
                    crown::TrustLevel::Trusted => "trusted".to_string(),
                    crown::TrustLevel::Observer => "observer".to_string(),
                    crown::TrustLevel::Untrusted => "untrusted".to_string(),
                };
                peer
            })
            .collect();
        MeshPeersResponse {
            paired,
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

    /// Whether this instance is the crown holder.
    pub fn is_crown_holder(&self) -> bool {
        self.crown_keypair.is_some()
    }

    /// Get the crown holder's instance ID (as known by this peer).
    pub fn crown_holder_id(&self) -> Option<String> {
        self.crown_public.as_ref().map(|c| c.holder_id.clone())
    }

    /// Claim crown if nobody has it. Called after first accept.
    pub fn claim_crown_if_unclaimed(&mut self) {
        if self.crown_keypair.is_some() || self.crown_public.is_some() {
            return; // Already have crown or know who does
        }
        log::info!("[mesh] No crown exists — claiming as first approver");
        let keypair = crown::CrownKeyPair::generate();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mesh_id = uuid::Uuid::new_v4().to_string();
        let claims = crown::CrownClaims {
            holder_id: self.identity.instance_id.clone(),
            mesh_id,
            crowned_at: now,
            version: keypair.version,
            emergency: false,
            voted_by: vec![],
            iat: now,
        };
        match crown::create_crown_jwt(&keypair, &claims) {
            Ok(jwt) => {
                let public_info = crown::CrownPublicInfo {
                    public_key: keypair.public_key.clone(),
                    crown_jwt: jwt,
                    version: keypair.version,
                    holder_id: self.identity.instance_id.clone(),
                };
                let _ = crown::save_crown_keypair(&keypair);
                let _ = crown::save_crown_public_info(&public_info);
                self.crown_keypair = Some(keypair);
                self.crown_public = Some(public_info);
                log::info!("[mesh] Claimed crown — this instance is now the leader");
            }
            Err(e) => log::error!("[mesh] Failed to create crown JWT: {}", e),
        }
    }

    /// Transfer crown to a paired peer. Only crown holder can call this.
    /// Returns the CrownPublicInfo to send to the target and all peers.
    pub fn transfer_crown(&mut self, target_id: &str) -> Result<crown::CrownPublicInfo, String> {
        let keypair = self.crown_keypair.as_ref()
            .ok_or("Only the crown holder can transfer the crown")?;
        if !self.paired_peers.contains_key(target_id) {
            return Err("Peer not found".to_string());
        }

        // Generate new keypair for the new holder
        let mut new_keypair = crown::CrownKeyPair::generate();
        new_keypair.version = keypair.version + 1;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mesh_id = self.crown_public.as_ref()
            .map(|c| {
                crown::verify_crown_jwt(&c.crown_jwt, &c.public_key)
                    .map(|cl| cl.mesh_id)
                    .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let claims = crown::CrownClaims {
            holder_id: target_id.to_string(),
            mesh_id,
            crowned_at: now,
            version: new_keypair.version,
            emergency: false,
            voted_by: vec![],
            iat: now,
        };
        let jwt = crown::create_crown_jwt(&new_keypair, &claims)?;
        let public_info = crown::CrownPublicInfo {
            public_key: new_keypair.public_key.clone(),
            crown_jwt: jwt,
            version: new_keypair.version,
            holder_id: target_id.to_string(),
        };

        // Relinquish our crown
        crown::delete_crown_keypair();
        self.crown_keypair = None;
        self.crown_public = Some(public_info.clone());
        let _ = crown::save_crown_public_info(&public_info);

        log::info!("[mesh] Crown transferred to {} (v{})", target_id, new_keypair.version);

        // Return new_keypair and public_info — caller sends to target
        // Store new_keypair temporarily for the transfer
        // The target will receive it encrypted via mesh token
        Ok(public_info)
    }

    /// Receive crown (called on the target instance during transfer).
    pub fn receive_crown(&mut self, keypair: crown::CrownKeyPair, public_info: crown::CrownPublicInfo) {
        let _ = crown::save_crown_keypair(&keypair);
        let _ = crown::save_crown_public_info(&public_info);
        self.crown_keypair = Some(keypair);
        self.crown_public = Some(public_info);
        log::info!("[mesh] Received crown — this instance is now the leader");
    }

    /// Update crown public info (called on all peers when crown transfers).
    pub fn update_crown_public(&mut self, public_info: crown::CrownPublicInfo) -> Result<(), String> {
        // Verify the JWT is valid
        crown::verify_crown_jwt(&public_info.crown_jwt, &public_info.public_key)?;
        // Accept only if version is higher
        if let Some(existing) = &self.crown_public {
            if public_info.version <= existing.version {
                return Err("Stale crown version".to_string());
            }
        }
        let _ = crown::save_crown_public_info(&public_info);
        self.crown_public = Some(public_info);
        Ok(())
    }

    /// Create a signed trust declaration for a peer.
    pub fn set_trust_level(&mut self, peer_id: &str, level: crown::TrustLevel) -> Result<crown::TrustDeclaration, String> {
        let keypair = self.crown_keypair.as_ref()
            .ok_or("Only the crown holder can set trust levels")?;
        let version = self.trust_declarations.iter()
            .filter(|d| d.peer_id == peer_id)
            .map(|d| d.version)
            .max()
            .unwrap_or(0) + 1;
        let decl = crown::create_trust_declaration(
            keypair, peer_id, level, &self.identity.instance_id, version,
        )?;
        // Replace existing declaration for this peer
        self.trust_declarations.retain(|d| d.peer_id != peer_id);
        self.trust_declarations.push(decl.clone());
        let _ = crown::save_trust_declarations(&self.trust_declarations);
        Ok(decl)
    }

    /// Get trust level for a peer. Crown holder is always trusted.
    pub fn get_trust_level(&self, peer_id: &str) -> crown::TrustLevel {
        // Crown holder is always trusted
        if self.crown_public.as_ref().map(|c| c.holder_id.as_str()) == Some(peer_id) {
            return crown::TrustLevel::Trusted;
        }
        self.trust_declarations.iter()
            .filter(|d| d.peer_id == peer_id)
            .max_by_key(|d| d.version)
            .map(|d| d.trust_level.clone())
            .unwrap_or(crown::TrustLevel::Untrusted)
    }

    /// Count trusted peers (for quorum calculation).
    pub fn trusted_peer_count(&self) -> usize {
        self.paired_peers.keys()
            .filter(|id| self.get_trust_level(id) == crown::TrustLevel::Trusted)
            .count()
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

    /// Authenticate a signed mesh request. Returns the sender's instance_id if valid.
    pub fn authenticate_request(&self, signed_token: &str) -> Option<String> {
        let known_keys = self.known_public_keys();
        match crown::verify_signed_request(signed_token, &known_keys) {
            Ok(peer_id) => {
                if !self.is_revoked(&peer_id) {
                    return Some(peer_id);
                }
                log::warn!("[mesh/auth] Peer {} is revoked", peer_id);
                None
            }
            Err(e) => {
                log::debug!("[mesh/auth] Signature verification failed: {}", e);
                None
            }
        }
    }

    /// Get map of instance_id → public_key for all known peers.
    pub fn known_public_keys(&self) -> HashMap<String, String> {
        self.paired_peers.iter()
            .map(|(id, peer)| (id.clone(), peer.public_key.clone()))
            .collect()
    }

    /// Sign a request with this instance's private key.
    pub fn sign_request(&self) -> Result<String, String> {
        let priv_key = self.identity.private_key.as_ref()
            .ok_or("No private key in identity")?;
        crown::sign_request(priv_key, &self.identity.instance_id)
    }

    /// Create an authenticated PeerClient for a paired peer.
    pub fn client_for(&self, peer_id: &str) -> Option<transport::PeerClient> {
        let peer = self.paired_peers.get(peer_id)?;
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())?;
        let token = self.sign_request().ok()?;
        Some(transport::PeerClient::new_signed(&addr.ip, peer.api_port, &token))
    }
}
