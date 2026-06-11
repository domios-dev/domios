use std::collections::HashMap;

use mdns_sd::ServiceDaemon;

use super::{
    crown, discovered, identity, peer_services, peers, transport,
};
use super::types::{
    self, DiscoveredPeer, MeshPeersResponse, MeshStatus, PairedPeer, PairingRequest,
    RevocationList,
};

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
    pub local_https_url: Option<String>,
    pub api_port: u16,
    pub pairing_targets: HashMap<String, (String, u16, String)>,
    pub mdns_daemon: Option<ServiceDaemon>,
    pub mdns_service_fullname: Option<String>,
    pub udp_started: bool,
    pub local_http_healthy: bool,
    pub disk_persist: bool,
    pub seen_nonces: HashMap<String, u64>,
    pub peer_services: peer_services::PeerServicesCache,
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
            local_https_url: None,
            api_port,
            pairing_targets: HashMap::new(),
            mdns_daemon: None,
            mdns_service_fullname: None,
            udp_started: false,
            local_http_healthy: true,
            disk_persist: true,
            seen_nonces: HashMap::new(),
            peer_services: peer_services::PeerServicesCache::default(),
        }
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
            build: env!("BUILD_HASH").to_string(),
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
            self.revocation_list.revoked.retain(|id| id != instance_id);
            log::info!("[mesh] Unpaired {}", instance_id);
            let _ = self.save();
        }
        removed
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

    pub fn upsert_discovered(&mut self, sighting: discovered::DiscoverySighting, now: u64) {
        if sighting.instance_id == self.ident.identity.instance_id {
            return;
        }
        let merged = discovered::merge_sighting(
            self.discovered_peers.get(&sighting.instance_id),
            sighting,
            now,
        );
        self.discovered_peers
            .insert(merged.instance_id.clone(), merged);
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

    pub fn build_sync_payload(&self) -> types::MeshSyncPayload {
        types::MeshSyncPayload {
            peers: self
                .paired_peers
                .values()
                .map(|p| types::GossipPeerInfo {
                    instance_id: p.instance_id.clone(),
                    instance_name: p.instance_name.clone(),
                    addresses: p.addresses.clone(),
                    api_port: p.api_port,
                    dev: false,
                })
                .collect(),
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
        let peer = self.paired_peers.get(peer_id)?;
        let addr = peer
            .addresses
            .iter()
            .min_by_key(|a| types::address_tier(a))?;
        let token = self.sign_request().ok()?;
        Some(transport::PeerClient::new_signed(
            &addr.ip,
            peer.api_port,
            &token,
        ))
    }

    pub fn local_announce_addresses(&self) -> Vec<types::PeerAddress> {
        let mut out = Vec::new();
        if let Some(ip) = &self.tailscale_ip {
            if !ip.is_empty() {
                out.push(types::PeerAddress {
                    ip: ip.clone(),
                    is_tailscale: true,
                });
            }
        }
        if let Some(ip) = &self.local_ip {
            if !ip.is_empty() && types::is_lan_private_ip(ip) {
                out.push(types::PeerAddress {
                    ip: ip.clone(),
                    is_tailscale: false,
                });
            }
        }
        out
    }

    pub fn build_announce(&self) -> types::PeerAnnounce {
        let online = self.local_http_healthy;
        types::PeerAnnounce {
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            addresses: if online {
                self.local_announce_addresses()
            } else {
                Vec::new()
            },
            api_port: self.api_port,
            online,
            https_url: if online {
                self.local_https_url.clone()
            } else {
                None
            },
        }
    }

    pub fn set_local_http_healthy(&mut self, healthy: bool) -> bool {
        let changed = self.local_http_healthy != healthy;
        self.local_http_healthy = healthy;
        changed
    }

    pub fn apply_announce(
        &mut self,
        sender_id: &str,
        ann: &types::PeerAnnounce,
    ) -> Result<Option<Vec<types::PeerAddress>>, String> {
        if sender_id != ann.instance_id {
            return Err(format!(
                "Sender {} cannot announce addresses for {}",
                sender_id, ann.instance_id
            ));
        }
        let clean = types::sanitize_addresses(&ann.addresses);
        if clean.is_empty() && ann.online {
            return Ok(None);
        }
        let (prev, online_changed) = {
            let peer = self
                .paired_peers
                .get_mut(&ann.instance_id)
                .ok_or_else(|| format!("Peer {} not paired", ann.instance_id))?;
            let prev = peer.addresses.clone();
            let online_changed = peer.online != ann.online;
            peer.addresses = clean.clone();
            peer.api_port = ann.api_port;
            peer.online = ann.online;
            peer.https_url = ann.https_url.clone();
            if !ann.instance_name.is_empty() {
                peer.instance_name = ann.instance_name.clone();
            }
            (prev, online_changed)
        };
        let _ = self.save();
        if prev_addrs_equal(&prev, &clean) && !online_changed {
            Ok(None)
        } else {
            Ok(Some(prev))
        }
    }

    pub fn set_peer_addr_override(
        &mut self,
        peer_id: &str,
        ip: &str,
        port: Option<u16>,
    ) -> Result<Vec<types::PeerAddress>, String> {
        let peer = self
            .paired_peers
            .get_mut(peer_id)
            .ok_or_else(|| format!("Peer {} not paired", peer_id))?;
        let prev = peer.addresses.clone();
        peer.addresses = vec![types::PeerAddress::from_ip(ip)];
        if let Some(p) = port {
            peer.api_port = p;
        }
        let _ = self.save();
        Ok(prev)
    }
}

fn prev_addrs_equal(a: &[types::PeerAddress], b: &[types::PeerAddress]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.ip == y.ip && x.is_tailscale == y.is_tailscale)
}

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
            https_url: None,
            consecutive_misses: 0,
        }
    }

    #[test]
    fn apply_offline_announce_clears_addresses_and_marks_peer_offline() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("peer-1".into(), paired_peer("peer-1"));

        let prev = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: false,
                    https_url: None,
                },
            )
            .unwrap();
        let peer = mesh.paired_peers.get("peer-1").unwrap();

        assert!(prev.is_some());
        assert!(peer.addresses.is_empty());
        assert!(!peer.online);
    }

    #[test]
    fn apply_online_empty_announce_keeps_existing_addresses() {
        let mut mesh = MeshManager::new("local".into(), 7685);
        mesh.disk_persist = false;
        mesh.paired_peers
            .insert("peer-1".into(), paired_peer("peer-1"));

        let prev = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: true,
                    https_url: None,
                },
            )
            .unwrap();
        let peer = mesh.paired_peers.get("peer-1").unwrap();

        assert!(prev.is_none());
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

        let prev = mesh
            .apply_announce(
                "peer-1",
                &types::PeerAnnounce {
                    instance_id: "peer-1".into(),
                    instance_name: "peer".into(),
                    addresses: Vec::new(),
                    api_port: 7685,
                    online: false,
                    https_url: None,
                },
            )
            .unwrap();

        assert!(prev.is_some());
        assert!(!mesh.paired_peers.get("peer-1").unwrap().online);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::{PairedPeer, PairingRequest, PeerAddress, PeerAnnounce};

    #[test]
    fn check_and_record_nonce_rejects_replay() {
        let mut mgr = MeshManager::new("test-node".to_string(), 7777);
        mgr.disk_persist = false;

        let (priv_key, pub_key) = crate::mesh::crown::generate_peer_keypair();
        mgr.ident.identity.private_key = Some(priv_key.clone());
        mgr.ident.identity.public_key = Some(pub_key.clone());
        let instance_id = mgr.ident.identity.instance_id.clone();

        let token = crate::mesh::crown::sign_request(&priv_key, &instance_id).unwrap();

        assert!(
            mgr.check_and_record_nonce(&token).is_ok(),
            "first use should succeed"
        );
        assert!(
            mgr.check_and_record_nonce(&token).is_err(),
            "replay should be rejected"
        );
    }

    fn pending_request(id: &str, from: &str, timestamp: u64) -> PairingRequest {
        PairingRequest {
            request_id: id.into(),
            from_instance_id: from.into(),
            from_instance_name: from.into(),
            from_address: "192.168.1.2".into(),
            from_port: 7685,
            from_addresses: vec![],
            public_key: "pk".into(),
            timestamp,
            status: types::PairingStatus::Pending,
            introduced_by: None,
        }
    }

    #[test]
    fn cleanup_marks_expired_then_drops_with_pairing_targets() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        m.pending_requests
            .insert("r1".into(), pending_request("r1", "peer-a", 1000));
        m.pairing_targets
            .insert("r1".into(), ("192.168.1.2".into(), 7685, "peer-a".into()));

        m.cleanup_expired_requests_at(1000 + 301);
        let r = m
            .pending_requests
            .get("r1")
            .expect("retained for visibility");
        assert_eq!(r.status, types::PairingStatus::Expired);
        assert!(m.pairing_targets.contains_key("r1"));
        assert_eq!(m.pending_request_count(), 0, "expired must not count");

        m.cleanup_expired_requests_at(1000 + 3601);
        assert!(m.pending_requests.is_empty());
        assert!(m.pairing_targets.is_empty(), "targets must not leak");
    }

    #[test]
    fn expired_request_does_not_block_resend() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        m.pending_requests
            .insert("r1".into(), pending_request("r1", "peer-a", 1000));
        m.cleanup_expired_requests_at(1000 + 301);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let fresh = pending_request("r2", "peer-a", now);
        assert!(
            m.receive_pairing_request(fresh).is_ok(),
            "re-send after expiry must be accepted"
        );
    }

    fn make_mesh_with_peer(peer_id: &str, addrs: Vec<&str>) -> MeshManager {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        let peer = PairedPeer {
            instance_id: peer_id.into(),
            instance_name: "TestPeer".into(),
            addresses: addrs.into_iter().map(PeerAddress::from_ip).collect(),
            api_port: 7685,
            public_key: String::new(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: types::AccessLevel::Full,
            version: None,
            https_url: None,
            consecutive_misses: 0,
        };
        m.paired_peers.insert(peer_id.into(), peer);
        m
    }

    #[test]
    fn apply_announce_replaces_address_list_wholesale() {
        let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
        let ann = PeerAnnounce {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![PeerAddress::from_ip("100.99.99.99")],
            api_port: 7777,
            online: true,
            https_url: None,
        };
        let prev = m
            .apply_announce("peer-A", &ann)
            .expect("ok")
            .expect("changed");
        assert_eq!(prev.len(), 2, "old list returned for logging");
        let after = &m.paired_peers["peer-A"];
        assert_eq!(after.addresses.len(), 1, "list fully replaced, not merged");
        assert_eq!(after.addresses[0].ip, "100.99.99.99");
        assert_eq!(after.api_port, 7777);
    }

    #[test]
    fn apply_announce_stores_and_clears_https_url() {
        let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
        let with_https = PeerAnnounce {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![PeerAddress::from_ip("100.64.0.5")],
            api_port: 7685,
            online: true,
            https_url: Some("https://peer-a.tail.ts.net".into()),
        };
        m.apply_announce("peer-A", &with_https).expect("ok");
        assert_eq!(
            m.paired_peers["peer-A"].https_url.as_deref(),
            Some("https://peer-a.tail.ts.net")
        );

        let without = PeerAnnounce {
            https_url: None,
            ..with_https
        };
        m.apply_announce("peer-A", &without).expect("ok");
        assert_eq!(m.paired_peers["peer-A"].https_url, None);
    }

    #[test]
    fn apply_announce_rejects_spoofed_sender() {
        let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
        let ann = PeerAnnounce {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![PeerAddress::from_ip("100.99.99.99")],
            api_port: 7685,
            online: true,
            https_url: None,
        };
        let err = m.apply_announce("peer-B", &ann).unwrap_err();
        assert!(err.contains("cannot announce"), "got: {}", err);
        assert_eq!(
            m.paired_peers["peer-A"].addresses[0].ip, "192.168.1.5",
            "untouched"
        );
    }

    #[test]
    fn apply_announce_empty_list_is_noop() {
        let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5"]);
        let ann = PeerAnnounce {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![],
            api_port: 7685,
            online: true,
            https_url: None,
        };
        let res = m.apply_announce("peer-A", &ann).expect("ok");
        assert!(res.is_none(), "empty list does not clear cache");
        assert_eq!(m.paired_peers["peer-A"].addresses.len(), 1);
    }

    #[test]
    fn apply_announce_same_addrs_returns_no_change() {
        let mut m = make_mesh_with_peer("peer-A", vec!["100.64.0.5"]);
        let ann = PeerAnnounce {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![PeerAddress::from_ip("100.64.0.5")],
            api_port: 7685,
            online: true,
            https_url: None,
        };
        let res = m.apply_announce("peer-A", &ann).expect("ok");
        assert!(res.is_none(), "identical list reports no change");
    }

    #[test]
    fn set_peer_addr_override_replaces_with_single_entry() {
        let mut m = make_mesh_with_peer("peer-A", vec!["192.168.1.5", "100.64.0.5"]);
        let prev = m
            .set_peer_addr_override("peer-A", "10.0.0.99", Some(8888))
            .expect("ok");
        assert_eq!(prev.len(), 2);
        let after = &m.paired_peers["peer-A"];
        assert_eq!(after.addresses.len(), 1);
        assert_eq!(after.addresses[0].ip, "10.0.0.99");
        assert_eq!(after.api_port, 8888);
    }

    #[test]
    fn set_peer_addr_override_unknown_peer_errors() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        let err = m
            .set_peer_addr_override("ghost", "10.0.0.1", None)
            .unwrap_err();
        assert!(err.contains("not paired"), "got: {}", err);
    }

    #[test]
    fn build_announce_includes_tailscale_and_lan() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        m.tailscale_ip = Some("100.65.0.30".into());
        m.local_ip = Some("192.168.1.5".into());
        let ann = m.build_announce();
        assert_eq!(ann.addresses.len(), 2);
        assert!(ann.addresses[0].is_tailscale, "tailscale first");
        assert_eq!(ann.addresses[0].ip, "100.65.0.30");
        assert_eq!(ann.addresses[1].ip, "192.168.1.5");
        assert!(!ann.addresses[1].is_tailscale);
        assert_eq!(ann.api_port, 7685);
    }

    #[test]
    fn build_announce_skips_public_local_ip() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        m.tailscale_ip = None;
        m.local_ip = Some("8.8.8.8".into());
        let ann = m.build_announce();
        assert!(ann.addresses.is_empty(), "public IPs are not advertised");
    }

    #[test]
    fn client_for_picks_tailscale_via_tier_ranking() {
        let mut m = MeshManager::new("test-host".into(), 7685);
        m.disk_persist = false;
        let peer = PairedPeer {
            instance_id: "peer-A".into(),
            instance_name: "TestPeer".into(),
            addresses: vec![
                PeerAddress {
                    ip: "8.8.8.8".into(),
                    is_tailscale: false,
                },
                PeerAddress {
                    ip: "192.168.1.2".into(),
                    is_tailscale: false,
                },
                PeerAddress {
                    ip: "100.65.0.30".into(),
                    is_tailscale: true,
                },
            ],
            api_port: 7685,
            public_key: String::new(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: types::AccessLevel::Full,
            version: None,
            https_url: None,
            consecutive_misses: 0,
        };
        m.paired_peers.insert("peer-A".into(), peer);

        let picked = m.paired_peers["peer-A"]
            .addresses
            .iter()
            .min_by_key(|a| types::address_tier(a))
            .unwrap();
        assert_eq!(picked.ip, "100.65.0.30");
    }
}
