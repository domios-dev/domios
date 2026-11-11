use serde::Serialize;
use std::sync::Arc;

use super::MeshManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionPhase {
    Discovering,
    Connecting,
    Verifying,
    Committing,
    Ready,
    Failed,
    Retrying,
    AuthRefused,
    MembershipExpired,
    PeerNeedsUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct PeerConnection {
    pub installation_id: String,
    pub phase: ConnectionPhase,
}

impl MeshManager {
    pub fn bind_connection_phases(&mut self, emit: Arc<dyn Fn(PeerConnection) + Send + Sync>) {
        self.connection_phase_observer = Some(emit);
    }

    pub fn connection_phases(&self) -> Vec<PeerConnection> {
        let pairs = self.account_mesh_pairs();
        let mut phases = self.connection_phases.clone();
        for peer in &pairs {
            let Some(installation) = peer.installation_id.as_ref() else {
                continue;
            };
            let phase = phases.get(installation);
            if !matches!(
                phase,
                Some(
                    ConnectionPhase::Discovering
                        | ConnectionPhase::Connecting
                        | ConnectionPhase::Verifying
                        | ConnectionPhase::Committing
                )
            ) && matches!(
                self.traffic_decision(&peer.instance_id),
                crate::account_mesh::gate::TrafficDecision::Blocked(
                    crate::account_mesh::gate::BlockReason::LocalMembershipExpired
                        | crate::account_mesh::gate::BlockReason::PeerMembershipExpired
                        | crate::account_mesh::gate::BlockReason::PeerMembershipMissing
                )
            ) {
                phases.insert(installation.clone(), ConnectionPhase::MembershipExpired);
            }
        }
        phases
            .iter()
            .map(|(id, phase)| PeerConnection {
                installation_id: id.clone(),
                phase: if *phase == ConnectionPhase::Ready
                    && !pairs.iter().any(|peer| {
                        peer.installation_id.as_deref() == Some(id.as_str()) && peer.online
                    }) {
                    ConnectionPhase::Failed
                } else {
                    *phase
                },
            })
            .collect()
    }

    pub fn set_connection_phase(&mut self, installation_id: &str, phase: ConnectionPhase) {
        if self.connection_phases.get(installation_id) == Some(&phase) {
            return;
        }
        self.connection_phases
            .insert(installation_id.to_owned(), phase);
        self.bump_gate();
        if let Some(emit) = &self.connection_phase_observer {
            emit(PeerConnection {
                installation_id: installation_id.to_owned(),
                phase,
            });
        }
    }

    pub fn peer_requests_allowed(&self, peer_id: &str) -> bool {
        self.peer_request_retry
            .get(peer_id)
            .is_none_or(|deadline| self.now() >= *deadline)
    }

    pub fn installation_request_allowed(&self, installation_id: &str) -> bool {
        self.account_mesh_pairs()
            .iter()
            .filter(|peer| peer.installation_id.as_deref() == Some(installation_id))
            .all(|peer| self.peer_requests_allowed(&peer.instance_id))
    }

    pub fn peer_auth_refused(&self, peer_id: &str) -> bool {
        self.peer_request_retry.contains_key(peer_id)
    }

    pub fn has_healthy_connection(&self, installation_id: &str) -> bool {
        self.paired_peers.values().any(|peer| {
            peer.online
                && !self
                    .account
                    .unavailable_installations
                    .contains(installation_id)
                && self.traffic_open(&peer.instance_id)
                && self
                    .links
                    .get(&peer.instance_id)
                    .is_some_and(|record| record.installation_id() == Some(installation_id))
        })
    }

    pub fn record_peer_inventory_status(
        &mut self,
        peer_id: &str,
        link_nonce: Option<&str>,
        status: u16,
    ) {
        let phase = match status {
            401 => ConnectionPhase::AuthRefused,
            426 => ConnectionPhase::PeerNeedsUpdate,
            _ => return,
        };
        let Some(record) = self.links.get(peer_id) else {
            return;
        };
        if record.link_nonce.as_deref() != link_nonce || !self.traffic_open(peer_id) {
            return;
        }
        let Some(installation) = record.installation_id().map(str::to_owned) else {
            return;
        };
        self.peer_request_retry
            .insert(peer_id.to_owned(), self.now().saturating_add(60));
        self.connection_phases.insert(installation, phase);
    }

    pub fn record_peer_http_status(
        &mut self,
        peer_id: &str,
        link_nonce: Option<&str>,
        status: u16,
    ) {
        if self
            .links
            .get(peer_id)
            .is_none_or(|record| record.link_nonce.as_deref() != link_nonce)
            || !self.traffic_open(peer_id)
        {
            return;
        }
        let installation = self
            .links
            .get(peer_id)
            .and_then(|record| record.installation_id())
            .map(str::to_owned);
        let Some(installation) = installation else {
            return;
        };
        let online = (200..=299).contains(&status);
        if let Some(peer) = self.paired_peers.get_mut(peer_id) {
            peer.online = online;
            if online {
                peer.last_seen = Some(chrono::Utc::now().to_rfc3339());
                peer.consecutive_misses = 0;
            }
        }
        let phase = match status {
            401 | 426 => {
                self.peer_request_retry
                    .insert(peer_id.to_owned(), self.now().saturating_add(60));
                if status == 426 {
                    ConnectionPhase::PeerNeedsUpdate
                } else {
                    ConnectionPhase::AuthRefused
                }
            }
            200..=299 => {
                self.peer_request_retry.remove(peer_id);
                ConnectionPhase::Ready
            }
            _ => ConnectionPhase::Failed,
        };
        self.bump_gate();
        self.set_connection_phase(&installation, phase);
    }

    pub fn retain_connection_phases(&mut self, retain: impl Fn(&str) -> bool) {
        self.connection_phases.retain(|id, _| retain(id));
    }

    pub fn clear_connection_phases(&mut self) {
        for id in self.connection_phases.keys().cloned().collect::<Vec<_>>() {
            self.set_connection_phase(&id, ConnectionPhase::Failed);
        }
        self.connection_phases.clear();
        self.peer_request_retry.clear();
    }
}
