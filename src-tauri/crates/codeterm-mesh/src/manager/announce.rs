use super::MeshManager;
use crate::types;

#[derive(Debug, Default)]
pub struct PeerAnnounceChange {
    pub previous_addresses: Vec<types::PeerAddress>,
    pub addresses_changed: bool,
    pub online_changed: bool,
    pub web_ui_changed: bool,
    pub metadata_changed: bool,
}

impl PeerAnnounceChange {
    pub fn changed(&self) -> bool {
        self.addresses_changed
            || self.online_changed
            || self.web_ui_changed
            || self.metadata_changed
    }
}

impl MeshManager {
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
        crate::reachability::advertised_addresses_with_verified_forward(
            &self.ready_listeners,
            &out,
            self.api_port,
            self.local_web_ui.as_ref(),
        )
    }

    pub fn build_announce(&self) -> types::PeerAnnounce {
        let addresses = self.local_announce_addresses();
        let online = self.local_http_healthy && !addresses.is_empty();
        types::PeerAnnounce {
            instance_id: self.ident.identity.instance_id.clone(),
            instance_name: self.ident.instance_name.clone(),
            addresses: if online { addresses } else { Vec::new() },
            api_port: self.api_port,
            online,
            web_ui: if online {
                self.local_web_ui.clone()
            } else {
                None
            },
            relay_protocol_version: types::RELAY_PROTOCOL_VERSION,
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
    ) -> Result<PeerAnnounceChange, String> {
        if sender_id != ann.instance_id {
            return Err(format!(
                "Sender {} cannot announce addresses for {}",
                sender_id, ann.instance_id
            ));
        }
        let clean = types::sanitize_addresses(&ann.addresses);
        let normalized_web_ui = if ann.online {
            ann.web_ui
                .clone()
                .and_then(crate::web_ui_endpoints::normalize)
        } else {
            None
        };
        let change = {
            let peer = self
                .paired_peers
                .get_mut(&ann.instance_id)
                .ok_or_else(|| format!("Peer {} not paired", ann.instance_id))?;
            let previous_addresses = peer.addresses.clone();
            let next_addresses = if clean.is_empty() && ann.online {
                previous_addresses.clone()
            } else {
                clean
            };
            let change = PeerAnnounceChange {
                addresses_changed: !addresses_equal(&previous_addresses, &next_addresses),
                online_changed: peer.online != ann.online,
                web_ui_changed: peer.web_ui != normalized_web_ui,
                metadata_changed: peer.api_port != ann.api_port
                    || peer.relay_protocol_version != ann.relay_protocol_version
                    || (!ann.instance_name.is_empty() && peer.instance_name != ann.instance_name),
                previous_addresses,
            };
            peer.addresses = next_addresses;
            peer.api_port = ann.api_port;
            peer.online = ann.online;
            peer.web_ui = normalized_web_ui;
            peer.relay_protocol_version = ann.relay_protocol_version;
            if !ann.instance_name.is_empty() {
                peer.instance_name = ann.instance_name.clone();
            }
            change
        };
        if change.changed() {
            let _ = self.save();
        }
        Ok(change)
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

fn addresses_equal(a: &[types::PeerAddress], b: &[types::PeerAddress]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.ip == y.ip && x.is_tailscale == y.is_tailscale)
}
