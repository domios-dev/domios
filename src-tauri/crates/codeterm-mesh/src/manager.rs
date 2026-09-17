use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

pub struct PairingDelivery {
    target_id: String,
    request_id: String,
    outcome: Mutex<Option<Result<(), String>>>,
    retained: Mutex<bool>,
    ready: tokio::sync::Notify,
}

impl PairingDelivery {
    pub fn new(target_id: String, request_id: String) -> Self {
        Self {
            target_id,
            request_id,
            outcome: Mutex::new(None),
            retained: Mutex::new(false),
            ready: tokio::sync::Notify::new(),
        }
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn claim(&self, outcome: Result<(), String>) -> bool {
        let mut stored = self.outcome.lock();
        if stored.is_some() {
            return false;
        }
        *stored = Some(outcome);
        true
    }

    pub fn retain(&self) -> bool {
        if self.outcome.lock().is_some() {
            return false;
        }
        let mut retained = self.retained.lock();
        if *retained {
            return false;
        }
        *retained = true;
        true
    }

    pub fn is_retained(&self) -> bool {
        *self.retained.lock()
    }

    pub fn outcome(&self) -> Result<(), String> {
        self.outcome
            .lock()
            .clone()
            .expect("pairing delivery has a claimed terminal outcome")
    }

    pub fn publish(&self) {
        self.ready.notify_waiters();
    }

    pub fn complete(&self, outcome: Result<(), String>) -> bool {
        if !self.claim(outcome) {
            return false;
        }
        self.publish();
        true
    }

    pub async fn wait(&self) -> Result<(), String> {
        loop {
            let notified = self.ready.notified();
            if let Some(outcome) = self.outcome.lock().clone() {
                return outcome;
            }
            if self.is_retained() {
                return Ok(());
            }
            notified.await;
        }
    }
}

use mdns_sd::ServiceDaemon;

use super::ports::{default_last_activity, LastActivityProvider};
use super::types::{
    self, DiscoveredPeer, MeshPeersResponse, MeshStatus, PairedPeer, PairingRequest, RevocationList,
};
use super::{crown, discovered, identity, peer_services, peers, transport};

pub const MAX_PAIRING_DELIVERIES: usize = 64;

mod announce;
mod pairing_correlation;
pub use announce::PeerAnnounceChange;

/// Central mesh state — composes identity with peer/discovery state.
/// Single Mutex — no lock ordering concerns.
pub struct MeshManager {
    pub ident: identity::IdentityManager,
    pub paired_peers: HashMap<String, PairedPeer>,
    pub discovered_peers: HashMap<String, DiscoveredPeer>,
    pub pending_requests: HashMap<String, PairingRequest>,
    pub revocation_list: RevocationList,
    pub discovery_active: bool,
    pub broadcast_active: bool,
    pub broadcast_expires_at: Option<u64>,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub local_web_ui: Option<types::WebUiAnnouncement>,
    pub api_port: u16,
    pub build_hash: String,
    pub pairing_targets: HashMap<String, (String, u16, String)>,
    pub pairing_deliveries: HashMap<String, Arc<PairingDelivery>>,
    pub mdns_daemon: Option<ServiceDaemon>,
    pub mdns_service_fullname: Option<String>,
    pub udp_started: bool,
    pub local_http_healthy: bool,
    pub ready_listeners: Vec<std::net::SocketAddr>,
    pub disk_persist: bool,
    pub seen_nonces: HashMap<String, u64>,
    gossip_generations: HashMap<String, u64>,
    gossip_tombstones: HashMap<(String, String), u64>,
    local_gossip_generation: u64,
    last_gossip_claim_ids: Vec<String>,
    pub peer_services: peer_services::PeerServicesCache,
    pub last_activity: Arc<dyn LastActivityProvider>,
}

impl MeshManager {
    pub fn confirm_pairing_target_identity(
        &mut self,
        request_id: &str,
        remote_instance_id: &str,
    ) -> Result<(), String> {
        uuid::Uuid::parse_str(remote_instance_id)
            .map_err(|_| "Peer returned an invalid instance identity".to_string())?;

        let expected_id = self
            .pairing_targets
            .get(request_id)
            .map(|(_, _, id)| id.clone())
            .ok_or_else(|| "Pairing target no longer exists".to_string())?;
        self.reject_untrusted_pairing_identity(remote_instance_id)?;
        if expected_id == remote_instance_id {
            return Ok(());
        }

        let is_manual = self
            .discovered_peers
            .get(&expected_id)
            .is_some_and(|peer| peer.discovery_method == types::DiscoveryMethod::Manual);
        if !is_manual {
            return Err(format!(
                "Peer identity mismatch: expected {}, got {}",
                expected_id, remote_instance_id
            ));
        }
        if self.discovered_peers.contains_key(remote_instance_id) {
            return Err(format!(
                "Peer identity conflicts with an existing discovery: {}",
                remote_instance_id
            ));
        }

        let mut peer = self
            .discovered_peers
            .remove(&expected_id)
            .ok_or_else(|| "Manual peer placeholder no longer exists".to_string())?;
        peer.instance_id = remote_instance_id.to_string();
        self.discovered_peers
            .insert(remote_instance_id.to_string(), peer);
        if let Some((_, _, target_id)) = self.pairing_targets.get_mut(request_id) {
            *target_id = remote_instance_id.to_string();
        }
        Ok(())
    }

    pub fn validate_pairing_accept_identity(
        &self,
        request_id: &str,
        remote_instance_id: &str,
    ) -> Result<(), String> {
        uuid::Uuid::parse_str(remote_instance_id)
            .map_err(|_| "Peer returned an invalid instance identity".to_string())?;
        let expected_id = self
            .pairing_targets
            .get(request_id)
            .map(|(_, _, id)| id.as_str())
            .ok_or_else(|| "Pairing target no longer exists".to_string())?;
        if expected_id != remote_instance_id {
            return Err(format!(
                "Peer identity mismatch: expected {}, got {}",
                expected_id, remote_instance_id
            ));
        }
        if remote_instance_id == self.ident.identity.instance_id {
            return Err("Cannot pair with this CodeTerm instance".to_string());
        }
        if self.paired_peers.contains_key(remote_instance_id) {
            return Err("Peer is already paired".to_string());
        }
        Ok(())
    }

    fn reject_untrusted_pairing_identity(&self, remote_instance_id: &str) -> Result<(), String> {
        if remote_instance_id == self.ident.identity.instance_id {
            return Err("Cannot pair with this CodeTerm instance".to_string());
        }
        if self.paired_peers.contains_key(remote_instance_id) {
            return Err("Peer is already paired".to_string());
        }
        if self.is_blocked(remote_instance_id) {
            return Err("Peer identity is blocked".to_string());
        }
        if self.is_revoked(remote_instance_id) {
            return Err("Peer identity is revoked".to_string());
        }
        Ok(())
    }

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

        let ident = identity::IdentityManager::new(identity, instance_name);

        Self {
            ident,
            discovered_peers: HashMap::new(),
            paired_peers,
            pending_requests: HashMap::new(),
            revocation_list,
            discovery_active: false,
            broadcast_active: false,
            broadcast_expires_at: None,
            tailscale_ip: None,
            local_ip: None,
            local_web_ui: None,
            api_port,
            build_hash: String::new(),
            pairing_targets: HashMap::new(),
            pairing_deliveries: HashMap::new(),
            mdns_daemon: None,
            mdns_service_fullname: None,
            udp_started: false,
            local_http_healthy: true,
            ready_listeners: Vec::new(),
            disk_persist: true,
            seen_nonces: HashMap::new(),
            gossip_generations: HashMap::new(),
            gossip_tombstones: HashMap::new(),
            local_gossip_generation: 0,
            last_gossip_claim_ids: Vec::new(),
            peer_services: peer_services::PeerServicesCache::default(),
            last_activity: default_last_activity(),
        }
    }

    pub fn with_last_activity(
        instance_name: String,
        api_port: u16,
        last_activity: Arc<dyn LastActivityProvider>,
    ) -> Self {
        let mut mgr = Self::new(instance_name, api_port);
        mgr.last_activity = last_activity;
        mgr
    }

    // ── Forwarding methods (minimize caller changes) ────────────────

    pub fn save(&self) -> Result<(), String> {
        if !self.disk_persist {
            return Ok(());
        }
        peers::save_peers(&self.paired_peers)?;
        peers::save_revocation_list(&self.revocation_list)?;
        Ok(())
    }

    pub fn check_and_record_nonce(&mut self, token: &str) -> Result<(), String> {
        let nonce = crown::nonce_from_token(token)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if self.seen_nonces.contains_key(&nonce) {
            return Err(format!("Replay detected: nonce {} already used", nonce));
        }
        self.seen_nonces.insert(nonce, now);
        self.seen_nonces
            .retain(|_, &mut ts| now.saturating_sub(ts) <= 310);
        Ok(())
    }

    pub fn sign_request(&self) -> Result<String, String> {
        self.ident.sign_request()
    }
    pub fn known_public_keys(&self) -> HashMap<String, String> {
        self.ident.known_public_keys(&self.paired_peers)
    }
    pub fn authenticate_request(&self, signed_token: &str) -> Option<String> {
        self.ident
            .authenticate_request(signed_token, &self.paired_peers)
    }

    // ── Status & peers ──────────────────────────────────────────────

    pub fn status(&self) -> MeshStatus {
        let discovered_count = self
            .discovered_peers
            .values()
            .filter(|d| !self.paired_peers.contains_key(&d.instance_id))
            .count();
        MeshStatus {
            enabled: true,
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            build: self.build_hash.clone(),
            discovery_active: self.discovery_active,
            broadcast_active: self.broadcast_active,
            broadcast_expires_at: self.broadcast_expires_at,
            tailscale_ip: self.tailscale_ip.clone(),
            local_ip: self.local_ip.clone(),
            api_port: self.api_port,
            paired_count: self.paired_peers.len(),
            discovered_count,
        }
    }

    pub fn peers_response(&self) -> MeshPeersResponse {
        let paired: Vec<PairedPeer> = self.paired_peers.values().cloned().collect();
        MeshPeersResponse {
            paired,
            discovered: self
                .discovered_peers
                .values()
                .filter(|d| !self.paired_peers.contains_key(&d.instance_id))
                .cloned()
                .collect(),
            incoming_requests: self
                .pending_requests
                .values()
                .filter(|r| {
                    r.status == types::PairingStatus::Pending
                        && r.from_instance_id != self.ident.identity.instance_id
                })
                .cloned()
                .collect(),
            local_instance_name: self.ident.instance_name.clone(),
            local_last_active_ms: self.last_activity.read_for_gossip(),
            local_dev: codeterm_paths::is_dev_instance(),
        }
    }

    pub fn is_blocked(&self, instance_id: &str) -> bool {
        self.revocation_list
            .blocked
            .contains(&instance_id.to_string())
    }

    pub fn is_revoked(&self, instance_id: &str) -> bool {
        self.revocation_list
            .revoked
            .contains(&instance_id.to_string())
            || self.revocation_list.tombstones.contains_key(instance_id)
    }

    fn tombstone_peer(&mut self, instance_id: &str) {
        self.revocation_list.next_tombstone_generation = self
            .revocation_list
            .next_tombstone_generation
            .saturating_add(1);
        self.revocation_list.tombstones.insert(
            instance_id.to_string(),
            self.revocation_list.next_tombstone_generation,
        );
        if !self
            .revocation_list
            .revoked
            .contains(&instance_id.to_string())
        {
            self.revocation_list.revoked.push(instance_id.to_string());
        }
    }

    pub fn complete_explicit_pairing(&mut self, peer: PairedPeer) {
        let instance_id = peer.instance_id.clone();
        self.supersede_stale_paired_records(&peer);
        self.revocation_list.revoked.retain(|id| id != &instance_id);
        self.revocation_list.blocked.retain(|id| id != &instance_id);
        self.revocation_list.tombstones.remove(&instance_id);
        self.paired_peers.insert(instance_id, peer);
    }

    /// A peer that re-minted its instance_id (fresh identity, same physical
    /// machine) re-pairs under a NEW id but the SAME instance_name and host
    /// address. Drop the old record here, at the moment we know for certain
    /// the name+host now belongs to the incoming one — otherwise it sits in
    /// `paired_peers` forever as a dead name-resolution candidate (#434).
    fn supersede_stale_paired_records(&mut self, incoming: &PairedPeer) {
        let stale_ids: Vec<String> = self
            .paired_peers
            .values()
            .filter(|existing| {
                existing.instance_id != incoming.instance_id
                    && existing
                        .instance_name
                        .eq_ignore_ascii_case(&incoming.instance_name)
                    && existing
                        .addresses
                        .iter()
                        .any(|a| incoming.addresses.iter().any(|b| b.ip == a.ip))
            })
            .map(|p| p.instance_id.clone())
            .collect();
        for id in stale_ids {
            self.paired_peers.remove(&id);
            self.peer_services.prune(&id);
            log::info!(
                "[mesh] superseded stale paired record {} for re-paired peer {} ({})",
                id,
                incoming.instance_name,
                incoming.instance_id
            );
        }
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_requests
            .values()
            .filter(|r| r.status == types::PairingStatus::Pending)
            .count()
    }

    // ── Peer management ─────────────────────────────────────────────

    pub fn unpair(&mut self, instance_id: &str) -> Option<PairedPeer> {
        let removed = self.paired_peers.remove(instance_id);
        if removed.is_some() {
            self.peer_services.prune(instance_id);
            self.tombstone_peer(instance_id);
            log::info!("[mesh] Unpaired {}", instance_id);
            let _ = self.save();
        }
        removed
    }

    pub fn cancel_outgoing_pairing(&mut self, request_id: &str) -> bool {
        if !self.pairing_targets.contains_key(request_id) {
            return false;
        }
        let delivery_key = self
            .pairing_deliveries
            .iter()
            .find(|(_, delivery)| delivery.request_id() == request_id)
            .map(|(target_id, _)| target_id.clone());
        let delivery =
            delivery_key.and_then(|target_id| self.pairing_deliveries.remove(&target_id));
        self.pending_requests.remove(request_id);
        self.pairing_targets.remove(request_id);
        if let Some(delivery) = delivery {
            delivery.complete(Err("Pairing request cancelled".to_string()));
        }
        true
    }

    pub fn exit_mesh(&mut self) -> Vec<(String, Vec<types::PeerAddress>, u16)> {
        let peers: Vec<_> = self
            .paired_peers
            .values()
            .map(|p| (p.instance_id.clone(), p.addresses.clone(), p.api_port))
            .collect();
        self.paired_peers.clear();
        self.peer_services.clear();
        let _ = self.save();
        log::info!("[mesh] Exited mesh — removed {} peers", peers.len());
        peers
    }

    // ── Discovery ingest ────────────────────────────────────────────

    pub fn upsert_discovered(&mut self, sighting: discovered::DiscoverySighting, now: u64) -> bool {
        if sighting.instance_id == self.ident.identity.instance_id {
            return false;
        }
        if self.is_blocked(&sighting.instance_id) || self.is_revoked(&sighting.instance_id) {
            self.discovered_peers.remove(&sighting.instance_id);
            return false;
        }
        let merged = discovered::merge_sighting(
            self.discovered_peers.get(&sighting.instance_id),
            sighting,
            now,
        );
        self.discovered_peers
            .insert(merged.instance_id.clone(), merged);
        true
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
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(request.timestamp) > 300 {
            return Err("Request expired".to_string());
        }
        if self.pending_request_count() >= 5 {
            return Err("Too many pending requests".to_string());
        }
        if self.pending_requests.contains_key(&request.request_id) {
            return Err("Duplicate request ID".to_string());
        }
        if self.pending_requests.values().any(|r| {
            r.from_instance_id == request.from_instance_id
                && r.status == types::PairingStatus::Pending
        }) {
            return Err("Already have pending request from this instance".to_string());
        }
        if self.paired_peers.contains_key(&request.from_instance_id) {
            return Err("Already paired".to_string());
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

    pub fn cleanup_expired_requests(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.cleanup_expired_requests_at(now);
    }

    pub fn cleanup_expired_requests_at(&mut self, now: u64) {
        const REQUEST_TTL_SECS: u64 = 300;
        const REQUEST_RETENTION_SECS: u64 = 3600;
        let mut expired = 0usize;
        for r in self.pending_requests.values_mut() {
            if r.status == types::PairingStatus::Pending
                && now.saturating_sub(r.timestamp) > REQUEST_TTL_SECS
            {
                r.status = types::PairingStatus::Expired;
                expired += 1;
            }
        }
        let expired_request_ids: Vec<String> = self
            .pending_requests
            .iter()
            .filter(|(_, request)| request.status != types::PairingStatus::Pending)
            .map(|(request_id, _)| request_id.clone())
            .collect();
        let expired_deliveries: Vec<(String, Arc<PairingDelivery>)> = self
            .pairing_deliveries
            .iter()
            .filter(|(_, delivery)| {
                expired_request_ids
                    .iter()
                    .any(|id| id == delivery.request_id())
            })
            .map(|(target_id, delivery)| (target_id.clone(), delivery.clone()))
            .collect();
        for (target_id, delivery) in &expired_deliveries {
            delivery.claim(Err("Pairing request expired".to_string()));
            self.pending_requests.remove(delivery.request_id());
            self.pairing_targets.remove(delivery.request_id());
            self.pairing_deliveries.remove(target_id);
        }
        for (_, delivery) in expired_deliveries {
            delivery.publish();
        }
        let before = self.pending_requests.len();
        self.pending_requests
            .retain(|_, r| now.saturating_sub(r.timestamp) <= REQUEST_RETENTION_SECS);
        let dropped = before - self.pending_requests.len();
        let pending = &self.pending_requests;
        self.pairing_targets
            .retain(|id, _| pending.contains_key(id));
        if expired > 0 || dropped > 0 {
            log::info!(
                "[mesh] Pairing requests: {} marked expired, {} dropped",
                expired,
                dropped
            );
        }
    }

    // ── Sync & transport ────────────────────────────────────────────

    pub fn apply_gossip_peer_info(&mut self, info: &types::GossipPeerInfo) {
        if let Some(p) = self.paired_peers.get_mut(&info.instance_id) {
            p.last_active_ms = if info.dev { None } else { info.last_active_ms };
        }
    }

    pub fn apply_gossip_snapshot(
        &mut self,
        voucher: &str,
        generation: u64,
        peers: &[types::GossipPeerInfo],
        now: u64,
    ) {
        if self
            .gossip_generations
            .get(voucher)
            .is_some_and(|known| generation < *known)
        {
            return;
        }

        let is_new_generation = self
            .gossip_generations
            .get(voucher)
            .is_none_or(|known| generation > *known);
        if is_new_generation {
            self.gossip_tombstones
                .retain(|(tombstone_voucher, _), _| tombstone_voucher != voucher);
            self.gossip_generations
                .insert(voucher.to_string(), generation);
        }

        let claimed: std::collections::HashSet<&str> =
            peers.iter().map(|peer| peer.instance_id.as_str()).collect();
        let removed: Vec<String> = self
            .discovered_peers
            .iter()
            .filter(|(_, peer)| peer.vouched_by.iter().any(|id| id == voucher))
            .filter(|(id, _)| !claimed.contains(id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in removed {
            self.gossip_tombstones
                .insert((voucher.to_string(), id.clone()), generation);
            let remove_peer = self.discovered_peers.get_mut(&id).is_some_and(|peer| {
                peer.vouched_by.retain(|known| known != voucher);
                peer.vouched_by.is_empty()
                    && peer.discovery_method == types::DiscoveryMethod::Gossip
            });
            if remove_peer {
                self.discovered_peers.remove(&id);
            }
        }

        for peer in peers {
            self.apply_gossip_peer_info(peer);
            if peer.instance_id == self.ident.identity.instance_id
                || self.paired_peers.contains_key(&peer.instance_id)
                || self
                    .gossip_tombstones
                    .get(&(voucher.to_string(), peer.instance_id.clone()))
                    .is_some_and(|tombstone| generation <= *tombstone)
            {
                continue;
            }
            self.upsert_discovered(
                discovered::DiscoverySighting {
                    instance_id: peer.instance_id.clone(),
                    instance_name: peer.instance_name.clone(),
                    addresses: peer.addresses.clone(),
                    api_port: peer.api_port,
                    method: types::DiscoveryMethod::Gossip,
                    dev: peer.dev,
                    mesh_id: None,
                    voucher: Some(voucher.to_string()),
                },
                now,
            );
        }
    }

    pub fn build_sync_payload(&mut self) -> types::MeshSyncPayload {
        let is_dev = codeterm_paths::is_dev_instance();
        let self_last = self.last_activity.read_for_gossip();
        let mut peers: Vec<types::GossipPeerInfo> = self
            .paired_peers
            .values()
            .map(|p| types::GossipPeerInfo {
                instance_id: p.instance_id.clone(),
                instance_name: p.instance_name.clone(),
                addresses: p.addresses.clone(),
                api_port: p.api_port,
                dev: false,
                last_active_ms: p.last_active_ms,
            })
            .collect();
        peers.push(types::GossipPeerInfo {
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            addresses: self.local_announce_addresses(),
            api_port: self.api_port,
            dev: is_dev,
            last_active_ms: self_last,
        });
        let mut claim_ids: Vec<String> =
            peers.iter().map(|peer| peer.instance_id.clone()).collect();
        claim_ids.sort();
        if claim_ids != self.last_gossip_claim_ids {
            self.local_gossip_generation = self.local_gossip_generation.saturating_add(1);
            self.last_gossip_claim_ids = claim_ids;
        }
        types::MeshSyncPayload {
            peers,
            generation: self.local_gossip_generation,
        }
    }

    pub fn broadcast_clients(&self) -> Vec<transport::PeerClient> {
        self.paired_peers
            .keys()
            .filter_map(|pid| self.client_for(pid))
            .collect()
    }

    pub fn broadcast_clients_except(&self, exclude_id: &str) -> Vec<transport::PeerClient> {
        self.paired_peers
            .keys()
            .filter(|pid| pid.as_str() != exclude_id)
            .filter_map(|pid| self.client_for(pid))
            .collect()
    }

    pub fn prepare_introductions(
        &self,
        new_peer_id: &str,
    ) -> Option<(PairedPeer, Vec<PairedPeer>, String, String)> {
        let new_peer = self.paired_peers.get(new_peer_id)?.clone();
        let existing: Vec<_> = self
            .paired_peers
            .values()
            .filter(|p| p.instance_id != new_peer_id)
            .cloned()
            .collect();
        let token = self.sign_request().ok()?;
        Some((
            new_peer,
            existing,
            self.ident.identity.instance_id.clone(),
            token,
        ))
    }

    pub fn client_for(&self, peer_id: &str) -> Option<transport::PeerClient> {
        if self.is_blocked(peer_id) || self.is_revoked(peer_id) {
            return None;
        }
        let peer = self.paired_peers.get(peer_id)?;
        let token = self.sign_request().ok()?;
        if let Some(url) = crate::web_ui_endpoints::preferred_https_url(peer.web_ui.as_ref()) {
            return Some(transport::PeerClient::new_signed_url(url, &token));
        }
        let addr = peer
            .addresses
            .iter()
            .min_by_key(|a| types::address_tier(a))?;
        Some(transport::PeerClient::new_signed(
            &addr.ip,
            peer.api_port,
            &token,
        ))
    }
}

#[cfg(test)]
mod identity_tests;

#[cfg(test)]
mod gossip_tests;

#[cfg(test)]
mod persistence_tests;

#[cfg(test)]
mod liveness_tests {
    use super::*;

    fn paired_peer(id: &str) -> PairedPeer {
        PairedPeer {
            instance_id: id.into(),
            instance_name: "peer".into(),
            addresses: vec![types::PeerAddress::from_ip("100.64.1.9")],
            api_port: 7685,
            public_key: "pub".into(),
            paired_at: "now".into(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: types::AccessLevel::Full,
            version: None,
            web_ui: None,
            relay_protocol_version: 0,
            consecutive_misses: 0,
            last_active_ms: None,
        }
    }

    #[test]
    fn build_sync_payload_includes_self_last_active() {
        let activity = crate::ports::RecordingLastActivity::new(cfg!(debug_assertions));
        let mut mesh = MeshManager::with_last_activity("local".into(), 7685, activity.clone());
        activity.mark();
        let payload = mesh.build_sync_payload();
        let self_entry = payload
            .peers
            .iter()
            .find(|p| p.instance_id == mesh.ident.identity.instance_id)
            .expect("self in gossip");
        assert!(!self_entry.dev || cfg!(debug_assertions));
        if cfg!(debug_assertions) {
            assert_eq!(self_entry.last_active_ms, None);
        } else {
            assert!(self_entry.last_active_ms.is_some());
        }
    }

    #[test]
    fn apply_gossip_peer_info_updates_last_active() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("peer-1".into(), paired_peer("peer-1"));
        mesh.apply_gossip_peer_info(&types::GossipPeerInfo {
            instance_id: "peer-1".into(),
            instance_name: "peer".into(),
            addresses: vec![types::PeerAddress::from_ip("100.64.1.9")],
            api_port: 7685,
            dev: false,
            last_active_ms: Some(42_000),
        });
        assert_eq!(mesh.paired_peers["peer-1"].last_active_ms, Some(42_000));
    }

    #[test]
    fn apply_offline_announce_clears_addresses_and_marks_peer_offline() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("peer-1".into(), paired_peer("peer-1"));

        let change = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: false,
                    web_ui: None,
                    relay_protocol_version: 0,
                },
            )
            .unwrap();
        let peer = mesh.paired_peers.get("peer-1").unwrap();

        assert!(change.changed());
        assert!(change.addresses_changed);
        assert!(change.online_changed);
        assert!(peer.addresses.is_empty());
        assert!(!peer.online);
    }

    #[test]
    fn apply_online_empty_announce_keeps_existing_addresses() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("peer-1".into(), paired_peer("peer-1"));

        let change = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: true,
                    web_ui: None,
                    relay_protocol_version: 0,
                },
            )
            .unwrap();
        let peer = mesh.paired_peers.get("peer-1").unwrap();

        assert!(!change.changed());
        assert_eq!(peer.addresses.len(), 1);
        assert!(peer.online);
    }

    #[test]
    fn apply_offline_announce_reports_change_when_addresses_already_empty() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        let mut peer = paired_peer("peer-1");
        peer.addresses.clear();
        mesh.paired_peers.insert("peer-1".into(), peer);

        let change = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: false,
                    web_ui: None,
                    relay_protocol_version: 0,
                },
            )
            .unwrap();

        assert!(change.changed());
        assert!(change.online_changed);
        assert!(!mesh.paired_peers.get("peer-1").unwrap().online);
    }
}

#[cfg(test)]
mod pairing_tests;

#[cfg(test)]
mod tests;
