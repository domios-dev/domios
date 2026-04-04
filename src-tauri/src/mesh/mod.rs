//! CodeTerm Mesh — peer-to-peer networking between CodeTerm instances.
//!
//! Architecture (SOLID):
//! - IdentityManager — signing, authentication, keypair operations
//! - CrownManager — crown lifecycle (claiming, transferring, accepting)
//! - TrustManager — inverted trust: default=Trusted, King only signs Untrusted declarations
//! - MeshManager — thin facade composing the above + peer/discovery state

pub mod types;
pub mod peers;
pub mod discovery;
pub mod pairing;
pub mod transport;
pub mod crown;
pub mod identity;
pub mod crown_manager;
pub mod trust;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use types::{DiscoveredPeer, MeshPeersResponse, MeshStatus, PairedPeer, PairingRequest, RevocationList};

pub type MeshState = Arc<Mutex<MeshManager>>;

/// Central mesh state — composes identity, crown, and trust sub-managers
/// with peer/discovery state. Single Mutex — no lock ordering concerns.
pub struct MeshManager {
    /// Identity and request signing.
    pub ident: identity::IdentityManager,
    /// Crown (leadership) lifecycle.
    pub crown: crown_manager::CrownManager,
    /// TTL-based trust declarations.
    pub trust: trust::TrustManager,
    /// Paired peers (keyed by instance_id).
    pub paired_peers: HashMap<String, PairedPeer>,
    /// Discovered (unpaired) peers.
    pub discovered_peers: HashMap<String, DiscoveredPeer>,
    /// Incoming pairing requests.
    pub pending_requests: HashMap<String, PairingRequest>,
    /// Blocked/revoked peers.
    pub revocation_list: RevocationList,
    pub discovery_active: bool,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    /// Outgoing pairing target info (request_id → (address, port, instance_id)).
    pub pairing_targets: HashMap<String, (String, u16, String)>,
}

impl MeshManager {
    pub fn new(instance_name: String, api_port: u16) -> Self {
        let identity = peers::load_or_create_identity();
        let paired_peers = peers::load_peers();
        let revocation_list = peers::load_revocation_list();

        let mut crown_mgr = crown_manager::CrownManager::new();

        // No peers = no mesh = crown is stale (leftover from previous mesh). Clean it.
        if paired_peers.is_empty() && crown_mgr.is_crown_holder() {
            log::info!("[mesh] No peers but crown exists — clearing stale crown");
            crown_mgr.relinquish();
        }

        log::info!(
            "[mesh] Initialized: id={}, name={}, {} paired peers, crown={}",
            identity.instance_id, instance_name, paired_peers.len(), crown_mgr.is_crown_holder()
        );

        let trust_mgr = trust::TrustManager::load_and_validate(
            crown_mgr.keypair.as_ref(),
            crown_mgr.public_info.as_ref().map(|c| c.public_key.as_str()),
            &identity.instance_id,
        );

        let ident = identity::IdentityManager::new(identity, instance_name);

        Self {
            ident,
            crown: crown_mgr,
            trust: trust_mgr,
            discovered_peers: HashMap::new(),
            paired_peers,
            pending_requests: HashMap::new(),
            revocation_list,
            discovery_active: false,
            tailscale_ip: None,
            local_ip: None,
            api_port,
            pairing_targets: HashMap::new(),
        }
    }

    // ── Forwarding methods (minimize caller changes) ────────────────

    /// Persist paired peers to disk.
    pub fn save(&self) -> Result<(), String> {
        peers::save_peers(&self.paired_peers)?;
        peers::save_revocation_list(&self.revocation_list)?;
        Ok(())
    }

    pub fn is_crown_holder(&self) -> bool { self.crown.is_crown_holder() }
    pub fn crown_holder_id(&self) -> Option<String> { self.crown.crown_holder_id() }

    pub fn sign_request(&self) -> Result<String, String> { self.ident.sign_request() }
    pub fn known_public_keys(&self) -> HashMap<String, String> {
        self.ident.known_public_keys(&self.paired_peers)
    }
    pub fn authenticate_request(&self, signed_token: &str) -> Option<String> {
        self.ident.authenticate_request(signed_token, &self.paired_peers)
    }

    pub fn claim_crown_if_unclaimed(&mut self) {
        self.crown.claim_if_unclaimed(&self.ident.identity.instance_id);
    }

    pub fn transfer_crown(&mut self, target_id: &str) -> Result<u64, String> {
        self.crown.transfer(target_id, &self.paired_peers)
    }

    pub fn accept_crown(&mut self, version: u64, from_id: &str) -> Result<crown::CrownPublicInfo, String> {
        self.crown.accept(version, from_id, &self.ident.identity.instance_id, &self.paired_peers)
    }

    pub fn update_crown_public(&mut self, public_info: crown::CrownPublicInfo) -> Result<(), String> {
        self.crown.update_public(public_info, &self.ident.identity.instance_id)
    }

    pub fn set_trust_level(&mut self, peer_id: &str, level: crown::TrustLevel) -> Result<crown::TrustDeclaration, String> {
        let keypair = self.crown.keypair.as_ref()
            .ok_or("Only the crown holder can set trust levels")?;
        self.trust.set_trust_level(keypair, peer_id, level, &self.ident.identity.instance_id)
    }

    pub fn get_trust_level(&self, peer_id: &str) -> crown::TrustLevel {
        self.trust.get_trust_level(peer_id, self.crown.crown_holder_id().as_deref())
    }

    pub fn trusted_peer_count(&self) -> usize {
        self.trust.trusted_peer_count(
            self.paired_peers.keys(),
            self.crown.crown_holder_id().as_deref(),
        )
    }

    // ── Status & peers ──────────────────────────────────────────────

    pub fn status(&self) -> MeshStatus {
        let trust = if self.is_crown_holder() {
            "crown".to_string()
        } else {
            match self.get_trust_level(&self.ident.identity.instance_id) {
                crown::TrustLevel::Trusted => "trusted".to_string(),
                crown::TrustLevel::Observer => "observer".to_string(),
                crown::TrustLevel::Untrusted => "untrusted".to_string(),
            }
        };
        MeshStatus {
            enabled: true,
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            build: env!("BUILD_HASH").to_string(),
            is_crown: self.is_crown_holder(),
            crown_holder: self.crown_holder_id(),
            discovery_active: self.discovery_active,
            tailscale_ip: self.tailscale_ip.clone(),
            local_ip: self.local_ip.clone(),
            api_port: self.api_port,
            can_manage: self.can_manage_mesh(),
            paired_count: self.paired_peers.len(),
            discovered_count: self.discovered_peers.len(),
            trust_level: trust,
        }
    }

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
            discovered: self.discovered_peers.values()
                .filter(|d| !self.paired_peers.contains_key(&d.instance_id))
                .cloned().collect(),
            incoming_requests: self.pending_requests.values()
                .filter(|r| {
                    r.status == types::PairingStatus::Pending
                        && r.from_instance_id != self.ident.identity.instance_id
                })
                .cloned().collect(),
        }
    }

    pub fn is_blocked(&self, instance_id: &str) -> bool {
        self.revocation_list.blocked.contains(&instance_id.to_string())
    }

    pub fn is_revoked(&self, instance_id: &str) -> bool {
        self.revocation_list.revoked.contains(&instance_id.to_string())
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_requests.values()
            .filter(|r| r.status == types::PairingStatus::Pending)
            .count()
    }

    // ── Peer management ─────────────────────────────────────────────

    pub fn unpair(&mut self, instance_id: &str) -> Option<PairedPeer> {
        let removed = self.paired_peers.remove(instance_id);
        if removed.is_some() {
            self.revocation_list.revoked.retain(|id| id != instance_id);
            self.trust.remove_peer(instance_id);
            log::info!("[mesh] Unpaired {} — trust revoked", instance_id);
            if self.paired_peers.is_empty() {
                log::info!("[mesh] Last peer removed — clearing mesh state");
                self.crown.relinquish();
                self.trust.clear();
                let path = peers::mesh_dir().join("crown_public.json");
                let _ = std::fs::remove_file(&path);
            }
            let _ = self.save();
        }
        removed
    }

    pub fn can_manage_mesh(&self) -> bool {
        if self.is_crown_holder() { return true; }
        self.get_trust_level(&self.ident.identity.instance_id) == crown::TrustLevel::Trusted
    }

    pub fn exit_mesh(&mut self) -> Vec<(String, Vec<types::PeerAddress>, u16)> {
        let peers: Vec<_> = self.paired_peers.values()
            .map(|p| (p.instance_id.clone(), p.addresses.clone(), p.api_port))
            .collect();
        self.crown.relinquish();
        self.paired_peers.clear();
        self.trust.clear();
        let _ = self.save();
        let path = peers::mesh_dir().join("crown_public.json");
        let _ = std::fs::remove_file(&path);
        log::info!("[mesh] Exited mesh — removed {} peers", peers.len());
        peers
    }

    // ── Pairing ─────────────────────────────────────────────────────

    pub fn receive_pairing_request(&mut self, request: PairingRequest) -> Result<(), String> {
        if request.from_instance_id == self.ident.identity.instance_id {
            return Err("Own request".to_string());
        }
        if self.is_blocked(&request.from_instance_id) {
            return Err("Instance is blocked".to_string());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        if now.saturating_sub(request.timestamp) > 300 {
            return Err("Request expired".to_string());
        }
        if self.pending_request_count() >= 5 {
            return Err("Too many pending requests".to_string());
        }
        if self.pending_requests.contains_key(&request.request_id) {
            return Err("Duplicate request ID".to_string());
        }
        if self.pending_requests.values().any(|r| r.from_instance_id == request.from_instance_id && r.status == types::PairingStatus::Pending) {
            return Err("Already have pending request from this instance".to_string());
        }
        if self.paired_peers.contains_key(&request.from_instance_id) {
            return Err("Already paired".to_string());
        }

        log::info!("[mesh] Incoming pairing request from {} ({})",
            request.from_instance_name, request.from_instance_id);
        self.pending_requests.insert(request.request_id.clone(), request);
        Ok(())
    }

    pub fn cleanup_expired_requests(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let before = self.pending_requests.len();
        self.pending_requests.retain(|_, r| now.saturating_sub(r.timestamp) <= 300);
        if self.pending_requests.len() < before {
            log::info!("[mesh] Cleaned up {} expired pairing requests", before - self.pending_requests.len());
        }
    }

    // ── Sync & transport ────────────────────────────────────────────

    pub fn build_sync_payload(&self) -> types::MeshSyncPayload {
        let valid_decls = if let Some(ref cp) = self.crown.public_info {
            self.trust.valid_declarations(&cp.public_key)
        } else {
            vec![]
        };
        types::MeshSyncPayload {
            peers: self.paired_peers.values()
                .map(|p| types::GossipPeerInfo {
                    instance_id: p.instance_id.clone(),
                    instance_name: p.instance_name.clone(),
                    addresses: p.addresses.clone(),
                    api_port: p.api_port,
                    dev: false,
                }).collect(),
            crown_public: self.crown.public_info.clone(),
            trust_declarations: valid_decls,
        }
    }

    pub fn broadcast_clients(&self) -> Vec<transport::PeerClient> {
        self.paired_peers.keys()
            .filter_map(|pid| self.client_for(pid))
            .collect()
    }

    pub fn broadcast_clients_except(&self, exclude_id: &str) -> Vec<transport::PeerClient> {
        self.paired_peers.keys()
            .filter(|pid| pid.as_str() != exclude_id)
            .filter_map(|pid| self.client_for(pid))
            .collect()
    }

    pub fn prepare_introductions(&self, new_peer_id: &str) -> Option<(types::PairedPeer, Vec<types::PairedPeer>, String, String)> {
        let new_peer = self.paired_peers.get(new_peer_id)?.clone();
        let existing: Vec<_> = self.paired_peers.values()
            .filter(|p| p.instance_id != new_peer_id)
            .cloned().collect();
        let token = self.sign_request().ok()?;
        Some((new_peer, existing, self.ident.identity.instance_id.clone(), token))
    }

    pub fn client_for(&self, peer_id: &str) -> Option<transport::PeerClient> {
        let peer = self.paired_peers.get(peer_id)?;
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())?;
        let token = self.sign_request().ok()?;
        Some(transport::PeerClient::new_signed(&addr.ip, peer.api_port, &token))
    }

    /// Get the crown holder's PeerClient for trust refresh requests.
    pub fn crown_client(&self) -> Option<transport::PeerClient> {
        let holder_id = self.crown.crown_holder_id()?;
        self.client_for(&holder_id)
    }
}

// ── Free functions (async, need mesh Arc) ───────────────────────────

/// Broadcast sync payload (crown info + trust declarations) to all paired peers.
pub async fn broadcast_sync(mesh: &MeshState) {
    let (payload, clients) = {
        let m = mesh.lock();
        let payload = m.build_sync_payload();
        let clients: Vec<_> = m.paired_peers.values().filter_map(|p| {
            let addr = p.addresses.iter().find(|a| a.is_tailscale)
                .or_else(|| p.addresses.first())?;
            let token = m.sign_request().ok()?;
            Some((addr.ip.clone(), p.api_port, token, p.instance_id.clone()))
        }).collect();
        (payload, clients)
    };

    if clients.is_empty() { return; }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
    for (ip, port, token, peer_id) in &clients {
        let url = format!("http://{}:{}/api/mesh/sync", ip, port);
        match http.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&payload)
            .send().await
        {
            Ok(resp) if resp.status().is_success() =>
                log::info!("[mesh/broadcast] Synced trust to {}", peer_id),
            Ok(resp) => {
                let body = resp.text().await.unwrap_or_default();
                log::warn!("[mesh/broadcast] Sync rejected by {}: {}", peer_id, body);
            }
            Err(e) => log::warn!("[mesh/broadcast] Sync failed for {}: {}", peer_id, e),
        }
    }
}

/// Refresh trust declarations from the King.
/// Crown holder: re-signs expiring declarations locally and broadcasts.
/// Non-crown: requests fresh declarations from crown holder.
pub async fn refresh_trust(mesh: &MeshState) {
    let (is_crown, expiring, crown_holder_id) = {
        let m = mesh.lock();
        let expiring = m.trust.declarations_needing_refresh();
        if expiring.is_empty() { return; }
        (m.is_crown_holder(), expiring, m.crown_holder_id())
    };

    if is_crown {
        // Crown holder: re-sign expiring Untrusted declarations with fresh TTL
        {
            let mut m = mesh.lock();
            for peer_id in &expiring {
                // Only Untrusted declarations exist — re-sign them
                let _ = m.set_trust_level(peer_id, crown::TrustLevel::Untrusted);
                log::debug!("[mesh/trust-refresh] Re-signed untrust for {}", peer_id);
            }
        }
        broadcast_sync(mesh).await;
    } else if let Some(_crown_id) = crown_holder_id {
        // Non-crown: request fresh declarations from crown
        let client = mesh.lock().crown_client();
        if let Some(client) = client {
            match client.request_trust_refresh(&expiring).await {
                Ok(fresh_decls) => {
                    let mut m = mesh.lock();
                    if let Some(ref cp) = m.crown.public_info {
                        let pk = cp.public_key.clone();
                        m.trust.merge_declarations(&fresh_decls, &pk);
                        log::info!("[mesh/trust-refresh] Refreshed {} declarations from crown", fresh_decls.len());
                    }
                }
                Err(e) => {
                    log::warn!("[mesh/trust-refresh] Crown offline — expired untrust declarations will revert to Trusted: {}", e);
                }
            }
        }
    }
}

/// Send bidirectional introductions between a new peer and all existing peers.
pub async fn broadcast_introductions(
    new_peer: types::PairedPeer,
    existing_peers: Vec<types::PairedPeer>,
    our_id: String,
    token: String,
) {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
    for existing in &existing_peers {
        // new → existing
        if let Some(addr) = existing.addresses.first() {
            let intro = types::PeerIntroduction {
                instance_id: new_peer.instance_id.clone(),
                instance_name: new_peer.instance_name.clone(),
                addresses: new_peer.addresses.clone(),
                api_port: new_peer.api_port,
                public_key: new_peer.public_key.clone(),
                introduced_by: our_id.clone(),
            };
            let url = format!("http://{}:{}/api/mesh/introduce", addr.ip, existing.api_port);
            match http.post(&url).header("Authorization", format!("Bearer {}", token))
                .json(&intro).send().await {
                Ok(resp) if resp.status().is_success() =>
                    log::info!("[mesh/introduce] {} → {}", new_peer.instance_name, existing.instance_name),
                Ok(resp) => {
                    let body = resp.text().await.unwrap_or_default();
                    log::warn!("[mesh/introduce] {} → {} rejected: {}", new_peer.instance_name, existing.instance_name, body);
                }
                Err(e) => log::warn!("[mesh/introduce] {} → {} error: {}", new_peer.instance_name, existing.instance_name, e),
            }
        }
        // existing → new
        if let Some(addr) = new_peer.addresses.first() {
            let intro = types::PeerIntroduction {
                instance_id: existing.instance_id.clone(),
                instance_name: existing.instance_name.clone(),
                addresses: existing.addresses.clone(),
                api_port: existing.api_port,
                public_key: existing.public_key.clone(),
                introduced_by: our_id.clone(),
            };
            let url = format!("http://{}:{}/api/mesh/introduce", addr.ip, new_peer.api_port);
            match http.post(&url).header("Authorization", format!("Bearer {}", token))
                .json(&intro).send().await {
                Ok(resp) if resp.status().is_success() =>
                    log::info!("[mesh/introduce] {} → {}", existing.instance_name, new_peer.instance_name),
                Ok(resp) => {
                    let body = resp.text().await.unwrap_or_default();
                    log::warn!("[mesh/introduce] {} → {} rejected: {}", existing.instance_name, new_peer.instance_name, body);
                }
                Err(e) => log::warn!("[mesh/introduce] {} → {} error: {}", existing.instance_name, new_peer.instance_name, e),
            }
        }
    }
}
