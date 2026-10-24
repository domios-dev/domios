mod account_admission;
mod account_gate;
mod link_approval;
mod pairing_observer;
use std::collections::HashMap;
use std::sync::Arc;

use super::account_mesh::{self, ledger::LinkLedger};
use super::ports::{default_last_activity, LastActivityProvider};
use super::types::{self, MeshPeersResponse, MeshStatus, PairedPeer, RevocationList};
use super::{crown, identity, peer_services, peers, transport};

mod announce;
pub use account_admission::{
    AdmissionContext, LinkConfirmation, LinkOrigin, PendingAdoption, PendingLink,
};
pub use account_gate::{blocked_message, wait_until_blocked};
pub use announce::PeerAnnounceChange;

/// Central mesh state — composes identity with peer/discovery state.
/// Single Mutex — no lock ordering concerns.
pub struct MeshManager {
    pub ident: identity::IdentityManager,
    pub paired_peers: HashMap<String, PairedPeer>,
    pub revocation_list: RevocationList,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub local_web_ui: Option<types::WebUiAnnouncement>,
    pub api_port: u16,
    pub build_hash: String,
    pub local_http_healthy: bool,
    pub ready_listeners: Vec<std::net::SocketAddr>,
    pub disk_persist: bool,
    pub seen_nonces: HashMap<String, u64>,
    local_gossip_generation: u64,
    /// Paired peers whose latest announce was never acknowledged; flushed when they reconnect.
    unacked_announce: std::collections::HashSet<String>,
    last_gossip_claim_ids: Vec<String>,
    pub peer_services: peer_services::PeerServicesCache,
    pub protocol_refusals: crate::protocol_refusals::ProtocolRefusals,
    pub last_activity: Arc<dyn LastActivityProvider>,
    pairing_observer: Option<pairing_observer::PairingObserver>,
    links: LinkLedger,
    account: account_mesh::AccountMeshState,
    account_authority: Option<Arc<dyn account_mesh::AccountMeshAuthority>>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    gate_generation: tokio::sync::watch::Sender<u64>,
    pending_links: HashMap<String, account_admission::PendingLink>,
    link_requests: HashMap<String, account_mesh::approval::LinkRequest>,
    pending_adoptions: HashMap<String, account_admission::PendingAdoption>,
    account_ready: bool,
    remote_access_epoch: u64,
    adoptions: HashMap<String, account_gate::AdoptionAttempt>,
    advertised_memberships: HashMap<String, String>,
}

impl MeshManager {
    pub fn record_announce_outcome(&mut self, peer_id: &str, acknowledged: bool) {
        if acknowledged {
            self.unacked_announce.remove(peer_id);
        } else {
            self.unacked_announce.insert(peer_id.to_string());
        }
    }

    pub fn has_unacked_announce(&self, peer_id: &str) -> bool {
        self.unacked_announce.contains(peer_id)
    }

    pub fn new(instance_name: String, api_port: u16) -> Self {
        let identity = peers::load_or_create_identity();
        let records = peers::load_mesh_records();
        let revocation_list = peers::load_revocation_list();
        let load = if records.ledger_held {
            super::account_mesh::ledger::PeersLoad::Held
        } else {
            records.load
        };
        let (links, migrate_now) =
            LinkLedger::migrate(records.ledger, &records.peers, &revocation_list, load);
        let persist_now = migrate_now || records.ledger_needs_persist;

        log::info!(
            "[mesh] Initialized: id={}, name={}, {} paired peers",
            identity.instance_id,
            instance_name,
            records.peers.len()
        );

        let mut manager = Self::in_memory(identity, instance_name, api_port);
        manager.paired_peers = records.peers;
        manager.revocation_list = revocation_list;
        manager.links = links;
        manager.disk_persist = true;
        if persist_now {
            if let Err(e) = manager.save() {
                log::warn!("[mesh] link provenance not persisted: {}", e);
            }
        }
        manager
    }

    pub fn in_memory(identity: types::MeshIdentity, instance_name: String, api_port: u16) -> Self {
        let ident = identity::IdentityManager::new(identity, instance_name);
        Self {
            ident,
            paired_peers: HashMap::new(),
            revocation_list: RevocationList::default(),
            tailscale_ip: None,
            local_ip: None,
            local_web_ui: None,
            api_port,
            build_hash: String::new(),
            local_http_healthy: true,
            ready_listeners: Vec::new(),
            disk_persist: false,
            seen_nonces: HashMap::new(),
            local_gossip_generation: 0,
            unacked_announce: std::collections::HashSet::new(),
            last_gossip_claim_ids: Vec::new(),
            peer_services: peer_services::PeerServicesCache::default(),
            protocol_refusals: Default::default(),
            last_activity: default_last_activity(),
            pairing_observer: None,
            links: LinkLedger::default(),
            account: account_mesh::AccountMeshState::default(),
            account_authority: None,
            clock: Arc::new(account_gate::system_now),
            gate_generation: tokio::sync::watch::channel(0).0,
            pending_links: HashMap::new(),
            link_requests: HashMap::new(),
            pending_adoptions: HashMap::new(),
            account_ready: true,
            remote_access_epoch: 0,
            adoptions: HashMap::new(),
            advertised_memberships: HashMap::new(),
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

    pub fn machine_count(&self) -> usize {
        self.paired_peers.len().saturating_add(1)
    }

    pub fn bind_pairing_observer(&mut self, observer: Arc<dyn Fn(usize) + Send + Sync>) {
        self.bind_pairing_observers(observer, Arc::new(|_| {}));
    }

    pub fn bind_pairing_observers(
        &mut self,
        observer: Arc<dyn Fn(usize) + Send + Sync>,
        counts: Arc<dyn Fn(usize) + Send + Sync>,
    ) {
        counts(self.machine_count());
        self.pairing_observer = Some(pairing_observer::PairingObserver::new(
            self.paired_peers.keys().cloned().collect(),
            observer,
            counts,
        ));
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_with(|| {
            if self.disk_persist {
                peers::save_peers(&self.paired_peers, &self.links)?;
                peers::save_revocation_list(&self.revocation_list)?;
            }
            Ok(())
        })
    }

    pub fn save_with(&self, persist: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        persist()?;
        if let Some(observer) = &self.pairing_observer {
            observer.committed(self.paired_peers.keys().cloned().collect());
        }
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
    /// Keys of paired peers whose edge is open now; every inbound peer check reads this.
    pub fn known_public_keys(&self) -> HashMap<String, String> {
        self.paired_peers
            .iter()
            .filter(|(id, _)| self.traffic_open(id))
            .map(|(id, peer)| (id.clone(), peer.public_key.clone()))
            .collect()
    }
    pub fn authenticate_request(&self, signed_token: &str) -> Option<String> {
        crown::verify_signed_request(signed_token, &self.known_public_keys()).ok()
    }

    // ── Status & peers ──────────────────────────────────────────────

    pub fn status(&self) -> MeshStatus {
        MeshStatus {
            enabled: true,
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            build: self.build_hash.clone(),
            tailscale_ip: self.tailscale_ip.clone(),
            local_ip: self.local_ip.clone(),
            api_port: self.api_port,
            paired_count: self.paired_peers.len(),
        }
    }

    pub fn peers_response(&self) -> MeshPeersResponse {
        let paired: Vec<PairedPeer> = self.paired_peers.values().cloned().collect();
        MeshPeersResponse {
            paired,
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

    pub fn unrevoke(&mut self, instance_id: &str) -> Result<(), String> {
        uuid::Uuid::parse_str(instance_id)
            .map_err(|_| "Peer identity must be a UUID".to_string())?;
        if !self.is_revoked(instance_id) {
            return Err("Peer identity is not revoked".to_string());
        }
        self.revocation_list.revoked.retain(|id| id != instance_id);
        self.revocation_list.tombstones.remove(instance_id);
        Ok(())
    }

    fn tombstone_peer(&mut self, instance_id: &str) {
        let generation = self.removal_watermark().saturating_add(1);
        self.revocation_list.next_tombstone_generation = generation;
        self.revocation_list
            .tombstones
            .insert(instance_id.to_string(), generation);
        if !self
            .revocation_list
            .revoked
            .contains(&instance_id.to_string())
        {
            self.revocation_list.revoked.push(instance_id.to_string());
        }
    }

    pub(crate) fn removal_watermark(&self) -> u64 {
        self.revocation_list
            .tombstones
            .values()
            .copied()
            .max()
            .unwrap_or_default()
            .max(self.revocation_list.next_tombstone_generation)
    }

    // ── Peer management ─────────────────────────────────────────────

    pub fn unpair(&mut self, instance_id: &str) -> Result<Option<PairedPeer>, String> {
        self.unpair_with(instance_id, |manager| manager.save())
    }

    fn unpair_with(
        &mut self,
        instance_id: &str,
        persist: impl FnOnce(&Self) -> Result<(), String>,
    ) -> Result<Option<PairedPeer>, String> {
        let Some(peer) = self.paired_peers.get(instance_id) else {
            return Ok(None);
        };
        let disconnect = self.make_disconnect(peer)?;
        let previous_disconnect = self
            .links
            .disconnects
            .insert(instance_id.into(), disconnect);
        let Some(removed) = self.paired_peers.remove(instance_id) else {
            return Ok(None);
        };
        let link = self.links.remove(instance_id);
        let revocations = self.revocation_list.clone();
        self.tombstone_peer(instance_id);
        if let Err(error) = persist(self) {
            self.paired_peers
                .insert(instance_id.to_string(), removed.clone());
            if let Some(link) = link {
                self.links.links.insert(instance_id.to_string(), link);
            }
            self.revocation_list = revocations;
            self.links.disconnects.remove(instance_id);
            if let Some(previous) = previous_disconnect {
                self.links.disconnects.insert(instance_id.into(), previous);
            }
            return Err(error);
        }
        self.peer_services.prune(instance_id);
        self.protocol_refusals.forget(instance_id);
        self.bump_gate();
        log::info!("[mesh] Unpaired {}", instance_id);
        Ok(Some(removed))
    }

    pub fn saved_disconnect(
        &self,
        instance_id: &str,
    ) -> Option<&crate::disconnect::SavedDisconnect> {
        self.links.disconnects.get(instance_id)
    }

    pub(crate) fn disconnect_keys(&self) -> HashMap<String, String> {
        self.links
            .disconnects
            .iter()
            .map(|(id, saved)| (id.clone(), saved.public_key.clone()))
            .collect()
    }

    pub fn exit_mesh(&mut self) -> Vec<(String, Vec<types::PeerAddress>, u16)> {
        let peers: Vec<_> = self
            .paired_peers
            .values()
            .map(|p| (p.instance_id.clone(), p.addresses.clone(), p.api_port))
            .collect();
        self.paired_peers.clear();
        self.peer_services.clear();
        self.links.clear();
        self.bump_gate();
        let _ = self.save();
        log::info!("[mesh] Exited mesh — removed {} peers", peers.len());
        peers
    }

    // ── Sync & transport ────────────────────────────────────────────

    pub fn apply_gossip_peer_info(&mut self, info: &types::GossipPeerInfo) {
        if let Some(p) = self.paired_peers.get_mut(&info.instance_id) {
            p.last_active_ms = if info.dev { None } else { info.last_active_ms };
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

    pub fn client_for(&self, peer_id: &str) -> Option<transport::PeerClient> {
        if self.is_blocked(peer_id) || self.is_revoked(peer_id) || !self.traffic_open(peer_id) {
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
            relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
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
                    relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
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
                    relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
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
        mesh.insert_legacy_pair_for_test(peer);

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
                    relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
                },
            )
            .unwrap();

        assert!(change.changed());
        assert!(change.online_changed);
        assert!(!mesh.paired_peers.get("peer-1").unwrap().online);
    }
}

#[cfg(test)]
mod tests;
