use std::collections::HashSet;

use super::{ConnectionPhase, MeshManager};
use crate::types::PairedPeer;

/// Freshness window for verified cloud-ingress relay evidence when no reverse
/// stream is currently attached. Cleared immediately on stream detach or relay failure.
pub const CLOUD_RELAY_EVIDENCE_TTL_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboundCloudLiveness {
    Online,
    Reconnecting,
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InboundCloudEvidence {
    pub traffic_open: bool,
    pub auth_refused: bool,
    pub installation_unavailable: bool,
    pub stream_attached: bool,
    pub has_cloud_endpoint: bool,
    pub last_relay_ok_at: Option<u64>,
    pub now_secs: u64,
}

pub fn evaluate_inbound_cloud_liveness(evidence: InboundCloudEvidence) -> InboundCloudLiveness {
    if !evidence.traffic_open || evidence.auth_refused || evidence.installation_unavailable {
        return InboundCloudLiveness::Offline;
    }
    if evidence.stream_attached {
        return InboundCloudLiveness::Online;
    }
    if let Some(at) = evidence.last_relay_ok_at {
        if evidence.now_secs.saturating_sub(at) <= CLOUD_RELAY_EVIDENCE_TTL_SECS {
            return InboundCloudLiveness::Online;
        }
    }
    if evidence.has_cloud_endpoint {
        InboundCloudLiveness::Reconnecting
    } else {
        InboundCloudLiveness::Offline
    }
}

impl MeshManager {
    pub fn peer_installation_id(&self, peer_id: &str) -> Option<&str> {
        self.links
            .get(peer_id)
            .and_then(|record| record.installation_id())
    }

    pub fn has_cloud_ingress_route(&self, installation_id: &str) -> bool {
        self.cloud_ingress_endpoint(installation_id).is_some()
            || self
                .endpoint_records
                .pinned_cloud_endpoints
                .contains_key(installation_id)
    }

    /// True when this paired peer is reached only by dialing its cloud ingress
    /// (home dials out; the peer has no direct/tailnet HTTP listen addresses).
    pub fn is_inbound_only_cloud_peer(&self, peer_id: &str) -> bool {
        let Some(peer) = self.paired_peers.get(peer_id) else {
            return false;
        };
        if !peer.addresses.is_empty() {
            return false;
        }
        let Some(installation_id) = self.peer_installation_id(peer_id) else {
            return false;
        };
        if self.endpoint_target(installation_id).is_some() {
            return false;
        }
        self.has_cloud_ingress_route(installation_id)
            || self.reverse_streams.contains_key(installation_id)
    }

    pub fn inbound_cloud_liveness(&self, peer_id: &str) -> Option<InboundCloudLiveness> {
        if !self.is_inbound_only_cloud_peer(peer_id) {
            return None;
        }
        let installation_id = self.peer_installation_id(peer_id)?;
        Some(self.inbound_cloud_liveness_for_installation(peer_id, installation_id))
    }

    pub(crate) fn inbound_cloud_liveness_for_installation(
        &self,
        peer_id: &str,
        installation_id: &str,
    ) -> InboundCloudLiveness {
        evaluate_inbound_cloud_liveness(InboundCloudEvidence {
            traffic_open: self.traffic_open(peer_id),
            auth_refused: self.peer_auth_refused(peer_id),
            installation_unavailable: self
                .account_state()
                .unavailable_installations
                .contains(installation_id),
            stream_attached: self.reverse_streams.contains_key(installation_id),
            has_cloud_endpoint: self.has_cloud_ingress_route(installation_id),
            last_relay_ok_at: self.cloud_relay_ok_at.get(installation_id).copied(),
            now_secs: self.now(),
        })
    }

    pub fn effective_peer_online(&self, peer: &PairedPeer) -> bool {
        if let Some(liveness) = self.inbound_cloud_liveness(&peer.instance_id) {
            return liveness == InboundCloudLiveness::Online;
        }
        let unavailable = self
            .peer_installation_id(&peer.instance_id)
            .is_some_and(|id| self.account_state().unavailable_installations.contains(id));
        peer.online
            && !unavailable
            && self.traffic_open(&peer.instance_id)
            && !self.peer_auth_refused(&peer.instance_id)
    }

    pub fn attached_reverse_streams(&self) -> HashSet<String> {
        self.reverse_streams.keys().cloned().collect()
    }

    pub fn installation_instance_bindings(&self) -> Vec<(String, String)> {
        self.links
            .links
            .iter()
            .filter_map(|(instance_id, record)| {
                let installation_id = record.installation_id()?;
                self.paired_peers
                    .contains_key(instance_id)
                    .then(|| (installation_id.to_owned(), instance_id.clone()))
            })
            .collect()
    }

    fn peer_ids_for_installation(&self, installation_id: &str) -> Vec<String> {
        self.paired_peers
            .keys()
            .filter(|peer_id| self.peer_installation_id(peer_id) == Some(installation_id))
            .cloned()
            .collect()
    }

    pub(crate) fn attach_reverse_stream(&mut self, installation_id: String, binding: [u8; 32]) {
        self.reverse_streams
            .insert(installation_id.clone(), binding);
        let peer_ids = self.peer_ids_for_installation(&installation_id);
        let now_rfc = chrono::Utc::now().to_rfc3339();
        let mut any_open = false;
        for peer_id in peer_ids {
            if !self.traffic_open(&peer_id) {
                continue;
            }
            any_open = true;
            self.peer_request_retry.remove(&peer_id);
            if let Some(peer) = self.paired_peers.get_mut(&peer_id) {
                peer.online = true;
                peer.last_seen = Some(now_rfc.clone());
                peer.consecutive_misses = 0;
            }
        }
        if any_open {
            self.set_connection_phase(&installation_id, ConnectionPhase::Ready);
        }
        self.bump_gate();
    }

    pub(crate) fn detach_reverse_stream(&mut self, installation_id: &str, binding: &[u8; 32]) {
        if self.reverse_streams.get(installation_id) != Some(binding) {
            return;
        }
        self.reverse_streams.remove(installation_id);
        self.cloud_relay_ok_at.remove(installation_id);
        let peer_ids = self.peer_ids_for_installation(installation_id);
        let has_endpoint = self.has_cloud_ingress_route(installation_id);
        let mut degraded_any = false;
        for peer_id in peer_ids {
            if self.is_inbound_only_cloud_peer(&peer_id)
                || self
                    .paired_peers
                    .get(&peer_id)
                    .is_some_and(|peer| peer.addresses.is_empty())
            {
                if let Some(peer) = self.paired_peers.get_mut(&peer_id) {
                    peer.online = false;
                }
                degraded_any = true;
            }
        }
        if degraded_any {
            let phase = if has_endpoint {
                ConnectionPhase::Retrying
            } else {
                ConnectionPhase::Failed
            };
            self.set_connection_phase(installation_id, phase);
        }
        self.bump_gate();
    }

    /// Records live relay outcome for an inbound-only cloud installation.
    /// Returns `true` when any paired peer's online state changed.
    pub fn record_cloud_relay_outcome(&mut self, installation_id: &str, ok: bool) -> bool {
        let peer_ids = self.peer_ids_for_installation(installation_id);
        let previous_online: Vec<(String, bool)> = peer_ids
            .iter()
            .filter(|peer_id| self.is_inbound_only_cloud_peer(peer_id))
            .filter_map(|peer_id| {
                self.paired_peers
                    .get(peer_id)
                    .map(|peer| (peer_id.clone(), self.effective_peer_online(peer)))
            })
            .collect();
        let now = self.now();
        if ok {
            self.cloud_relay_ok_at
                .insert(installation_id.to_owned(), now);
        } else {
            self.cloud_relay_ok_at.remove(installation_id);
        }
        let mut changed = false;
        let now_rfc = ok.then(|| chrono::Utc::now().to_rfc3339());
        let mut target_phase = None;
        for (peer_id, was_online) in previous_online {
            let liveness = self.inbound_cloud_liveness_for_installation(&peer_id, installation_id);
            let next_online = liveness == InboundCloudLiveness::Online;
            if next_online {
                self.peer_request_retry.remove(&peer_id);
            }
            if let Some(peer) = self.paired_peers.get_mut(&peer_id) {
                if was_online != next_online {
                    changed = true;
                }
                peer.online = next_online;
                if let Some(ref seen) = now_rfc {
                    peer.last_seen = Some(seen.clone());
                    peer.consecutive_misses = 0;
                }
            }
            target_phase = Some(match liveness {
                InboundCloudLiveness::Online => ConnectionPhase::Ready,
                InboundCloudLiveness::Reconnecting => ConnectionPhase::Retrying,
                InboundCloudLiveness::Offline => ConnectionPhase::Failed,
            });
        }
        if let Some(phase) = target_phase {
            self.set_connection_phase(installation_id, phase);
        }
        if changed {
            self.bump_gate();
        }
        changed
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_reverse_stream_for_test(&mut self, installation_id: String, binding: [u8; 32]) {
        self.attach_reverse_stream(installation_id, binding);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn detach_reverse_stream_for_test(&mut self, installation_id: &str, binding: &[u8; 32]) {
        self.detach_reverse_stream(installation_id, binding);
    }
}

#[cfg(test)]
mod tests;
