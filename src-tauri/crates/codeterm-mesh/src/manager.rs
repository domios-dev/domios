mod account_admission;
mod account_gate;
mod connection_observer;
use std::collections::HashMap;
use std::sync::Arc;

use super::account_mesh::{self, ledger::LinkLedger};
use super::ports::{default_last_activity, LastActivityProvider};
use super::types::{self, MeshPeersResponse, MeshStatus, PairedPeer};
use super::{crown, identity, peer_services, peers, transport};

mod announce;
pub use account_admission::{AdmissionContext, LinkConfirmation, PendingLink};
pub use account_gate::{blocked_message, wait_until_blocked};
pub use announce::PeerAnnounceChange;

/// Central mesh state — composes identity with peer/discovery state.
/// Single Mutex — no lock ordering concerns.
pub struct MeshManager {
    pub ident: identity::IdentityManager,
    pub paired_peers: HashMap<String, PairedPeer>,
    pub account_installations: Vec<types::AccountInstallation>,
    pub tailscale_ip: Option<String>,
    pub network_transport: Option<crate::network_transport::TransportStatus>,
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
    connection_observer: Option<connection_observer::ConnectionObserver>,
    links: LinkLedger,
    account: account_mesh::AccountMeshState,
    account_authority: Option<Arc<dyn account_mesh::AccountMeshAuthority>>,
    pub registry_revision: Option<types::RegistryRevision>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    gate_generation: tokio::sync::watch::Sender<u64>,
    pending_links: HashMap<String, account_admission::PendingLink>,
    account_ready: bool,
    remote_access_epoch: u64,
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
        let removed = if cfg!(any(test, feature = "test-support")) {
            peers::LegacyPairRemoval::default()
        } else {
            peers::remove_legacy_pair_store()
        };
        for path in removed.removed {
            log::info!("[mesh] Removed legacy pair store {}", path.display());
        }
        for (path, error) in removed.failed {
            log::warn!(
                "[mesh] Could not remove legacy pair store {}: {}",
                path.display(),
                error
            );
        }
        let mut manager = Self::in_memory(identity, instance_name, api_port);
        manager.disk_persist = true;
        manager
    }

    pub fn in_memory(identity: types::MeshIdentity, instance_name: String, api_port: u16) -> Self {
        let ident = identity::IdentityManager::new(identity, instance_name);
        Self {
            ident,
            paired_peers: HashMap::new(),
            account_installations: Vec::new(),
            tailscale_ip: None,
            network_transport: None,
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
            connection_observer: None,
            links: LinkLedger::default(),
            account: account_mesh::AccountMeshState::default(),
            account_authority: None,
            registry_revision: None,
            clock: Arc::new(account_gate::system_now),
            gate_generation: tokio::sync::watch::channel(0).0,
            pending_links: HashMap::new(),
            account_ready: true,
            remote_access_epoch: 0,
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

    pub fn bind_connection_observer(&mut self, observer: Arc<dyn Fn(usize) + Send + Sync>) {
        self.bind_connection_observers(observer, Arc::new(|_| {}));
    }

    pub fn bind_connection_observers(
        &mut self,
        observer: Arc<dyn Fn(usize) + Send + Sync>,
        counts: Arc<dyn Fn(usize) + Send + Sync>,
    ) {
        counts(self.machine_count());
        self.connection_observer = Some(connection_observer::ConnectionObserver::new(
            self.paired_peers.keys().cloned().collect(),
            observer,
            counts,
        ));
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_with(|| Ok(()))
    }

    pub fn save_with(&self, persist: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        persist()?;
        if let Some(observer) = &self.connection_observer {
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
            transport: self.network_transport.clone(),
            web_ui: self.local_web_ui.clone(),
        }
    }

    pub fn peers_response(&self) -> MeshPeersResponse {
        let paired: Vec<PairedPeer> = self
            .paired_peers
            .values()
            .filter(|peer| {
                self.account.signed_in_account().is_some()
                    && self
                        .links
                        .get(&peer.instance_id)
                        .and_then(|record| record.installation_id())
                        .is_some_and(|id| {
                            self.account_installations
                                .iter()
                                .any(|row| row.installation_id == id && row.active)
                        })
            })
            .cloned()
            .collect();
        let mut installations: std::collections::BTreeMap<_, _> = self
            .account_installations
            .iter()
            .cloned()
            .map(|row| (row.installation_id.clone(), row))
            .collect();
        for connection in self.account_mesh_pairs() {
            if let Some(row) = connection
                .installation_id
                .as_ref()
                .and_then(|id| installations.get_mut(id))
            {
                row.instance_id = Some(connection.instance_id);
                row.online = connection.online && row.active && !row.signed_out;
            }
        }
        MeshPeersResponse {
            installations: installations.into_values().collect(),
            paired,
            local_instance_name: self.ident.instance_name.clone(),
            local_last_active_ms: self.last_activity.read_for_gossip(),
            local_dev: codeterm_paths::is_dev_instance(),
        }
    }

    pub fn account_peer_revoked(&self, instance_id: &str) -> bool {
        self.links
            .get(instance_id)
            .and_then(|record| record.installation_id())
            .is_some_and(|installation| self.account.revoked_installations.contains(installation))
    }

    // ── Peer management ─────────────────────────────────────────────

    pub fn remove_account_peer(&mut self, instance_id: &str) {
        self.paired_peers.remove(instance_id);
        self.links.remove(instance_id);
        self.peer_services.prune(instance_id);
        self.protocol_refusals.forget(instance_id);
        self.bump_gate();
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
        if !self.traffic_open(peer_id) {
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
mod liveness_tests;

#[cfg(test)]
mod tests;
