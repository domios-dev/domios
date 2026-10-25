use std::collections::HashMap;
use std::sync::Arc;

use super::MeshManager;
use crate::account_mesh::gate::{self, BlockReason, TrafficDecision};
use crate::account_mesh::ledger::{LinkAdmission, LinkRecord};
use crate::account_mesh::protocol::AccountMeshPair;
use crate::account_mesh::{
    AccountMeshAuthority, AccountMeshState, AccountPresence, AdmissionRefusal, SignedMembership,
};
use crate::MeshState;

pub(super) enum AdoptionAttempt {
    InFlight,
    Refused {
        account_epoch: u64,
        advertised: Option<String>,
    },
}

pub(super) fn system_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl MeshManager {
    /// Applies an account transition. Blocking takes effect before any caller-side cleanup.
    pub fn apply_account_state(&mut self, state: AccountMeshState) {
        if state != self.account {
            self.adoptions.clear();
        }
        if state.account_epoch != self.account.account_epoch {
            self.pending_links.clear();
            self.link_requests.clear();
            self.pending_adoptions.clear();
        }
        let bound = state
            .signed_in_account()
            .is_some_and(|account| self.links.bind_owner(account));
        if self.account_ready && self.account == state && !bound {
            return;
        }
        if matches!(state.presence, AccountPresence::SignedOut) {
            self.remote_access_epoch = self.remote_access_epoch.wrapping_add(1);
        }
        self.account = state;
        self.account_ready = true;
        if bound {
            if let Err(e) = self.save() {
                log::warn!("[mesh/account] owner binding not persisted: {}", e);
            }
        }
        self.bump_gate();
    }

    pub fn account_state(&self) -> &AccountMeshState {
        &self.account
    }

    pub fn remote_access_epoch(&self) -> Option<u64> {
        (self.account_ready && !matches!(self.account.presence, AccountPresence::SignedOut))
            .then_some(self.remote_access_epoch)
    }

    pub fn bind_account_authority(&mut self, authority: Arc<dyn AccountMeshAuthority>) {
        self.account_authority = Some(authority);
    }

    pub fn account_authority(&self) -> Option<Arc<dyn AccountMeshAuthority>> {
        self.account_authority.clone()
    }

    pub fn set_clock(&mut self, clock: Arc<dyn Fn() -> u64 + Send + Sync>) {
        self.clock = clock;
        self.bump_gate();
    }

    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    pub fn link_record(&self, peer_id: &str) -> Option<&LinkRecord> {
        self.links.get(peer_id)
    }

    pub fn owner_account(&self) -> Option<&str> {
        self.links.owner_account.as_deref()
    }

    /// Closes every edge until the account owner publishes its first state, so a persisted
    /// sign-out is never bypassed during startup.
    pub fn await_account_state(&mut self) {
        self.account_ready = false;
        self.remote_access_epoch = self.remote_access_epoch.wrapping_add(1);
        self.bump_gate();
    }

    pub fn traffic_decision(&self, peer_id: &str) -> TrafficDecision {
        if !self.account_ready {
            return TrafficDecision::Blocked(BlockReason::AccountPending);
        }
        let Some(peer) = self.paired_peers.get(peer_id) else {
            return TrafficDecision::Blocked(BlockReason::NoProvenance);
        };
        gate::decide(
            self.links.get(peer_id),
            &peer.public_key,
            self.links.owner_account.as_deref(),
            &self.account,
            self.now(),
        )
    }

    pub fn traffic_open(&self, peer_id: &str) -> bool {
        self.traffic_decision(peer_id).is_open()
    }

    pub fn admission_deadline(&self, peer_id: &str) -> Option<u64> {
        gate::admission_deadline(self.links.get(peer_id), &self.account)
    }

    /// Changes whenever any edge may have changed admission; long-lived channels re-decide on it.
    pub fn gate_watch(&self) -> tokio::sync::watch::Receiver<u64> {
        self.gate_generation.subscribe()
    }

    pub(crate) fn bump_gate(&self) {
        self.gate_generation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// The single endpoint the account registry may publish: this instance's Tailnet IP on a
    /// listener that is actually ready for remote traffic. `gate_watch` fires when it may change.
    pub fn ready_tailnet_endpoint(&self) -> Option<(std::net::IpAddr, u16)> {
        let status = self.network_transport.as_ref()?;
        crate::network_transport::listener_addresses(status, true, self.api_port)
            .into_iter()
            .find(|address| self.ready_listeners.contains(address))
            .map(|address| (address.ip(), address.port()))
    }

    pub fn set_network_transport(
        &mut self,
        status: crate::network_transport::TransportStatus,
    ) -> bool {
        if self.network_transport.as_ref() != Some(&status) {
            self.tailscale_ip =
                crate::network_transport::listener_addresses(&status, true, self.api_port)
                    .iter()
                    .map(|address| address.ip())
                    .find(|ip| ip.is_ipv4())
                    .map(|ip| ip.to_string());
            self.network_transport = Some(status);
            self.bump_gate();
            return true;
        }
        false
    }

    pub fn set_ready_listeners(&mut self, ready: Vec<std::net::SocketAddr>) -> bool {
        let changed = self.ready_listeners != ready;
        self.ready_listeners = ready;
        if changed {
            self.bump_gate();
        }
        changed
    }

    pub fn set_tailscale_ip(&mut self, ip: Option<String>) {
        if self.tailscale_ip != ip {
            self.tailscale_ip = ip;
            self.bump_gate();
        }
    }

    /// Outbound signing for one peer; refuses while that edge is blocked.
    pub fn sign_request_for(&self, peer_id: &str) -> Result<String, String> {
        match self.traffic_decision(peer_id) {
            TrafficDecision::Open => self.sign_request(),
            TrafficDecision::Blocked(reason) => Err(blocked_message(reason)),
        }
    }

    /// An announce may still go out when only the peer's view of our membership is stale.
    pub fn announce_allowed(&self, peer_id: &str) -> bool {
        matches!(
            self.traffic_decision(peer_id),
            TrafficDecision::Open
                | TrafficDecision::Blocked(
                    BlockReason::PeerMembershipExpired | BlockReason::PeerMembershipMissing
                )
        )
    }

    /// Identity keys for every paired peer regardless of admission. Only an
    /// announce may use them, and only to carry a renewed membership.
    pub fn paired_identity_keys(&self) -> HashMap<String, String> {
        self.paired_peers
            .iter()
            .map(|(id, peer)| (id.clone(), peer.public_key.clone()))
            .collect()
    }

    /// Accepts a renewed same-identity membership for an adopted edge; never adopts.
    pub fn refresh_peer_membership(&mut self, peer_id: &str, token: &str) -> bool {
        if self.account.signed_in_account().is_none() {
            return false;
        }
        let Some(authority) = self.account_authority.clone() else {
            return false;
        };
        let Ok(verified) = authority.verify_membership(token, self.now()) else {
            return false;
        };
        let renewed = SignedMembership {
            token: token.to_string(),
            verified,
        };
        if !self.links.refresh_remote_membership(peer_id, renewed) {
            return false;
        }
        if let Err(e) = self.save() {
            log::warn!("[mesh/account] renewed membership not persisted: {}", e);
        }
        self.bump_gate();
        true
    }

    /// Own membership token to advertise on announces, only while it is current.
    pub fn advertised_membership(&self) -> Option<String> {
        self.account
            .current_membership(self.now())
            .map(|membership| membership.token.clone())
    }

    pub fn account_mesh_pairs(&self) -> Vec<AccountMeshPair> {
        let mut pairs: Vec<AccountMeshPair> = self
            .paired_peers
            .values()
            .filter_map(|peer| {
                let record = self.links.get(&peer.instance_id)?;
                Some(AccountMeshPair {
                    instance_id: peer.instance_id.clone(),
                    installation_id: record.installation_id().map(str::to_string),
                    admission: record.admission.label().to_string(),
                    online: peer.online,
                    name: peer.instance_name.clone(),
                })
            })
            .collect();
        pairs.sort_by(|a, b| a.name.cmp(&b.name).then(a.instance_id.cmp(&b.instance_id)));
        pairs
    }

    /// Remembers the membership a peer last advertised; a change re-enables adoption offers.
    pub fn note_advertised_membership(&mut self, peer_id: &str, token: &str) {
        self.advertised_memberships
            .insert(peer_id.to_string(), token.to_string());
    }

    pub(crate) fn begin_adoption(&mut self, peer_id: &str) -> bool {
        let advertised = self.advertised_memberships.get(peer_id).cloned();
        let blocked = match self.adoptions.get(peer_id) {
            Some(AdoptionAttempt::InFlight) => true,
            Some(AdoptionAttempt::Refused {
                account_epoch,
                advertised: seen,
            }) => *account_epoch == self.account.account_epoch && *seen == advertised,
            None => false,
        };
        if !blocked {
            self.adoptions
                .insert(peer_id.to_string(), AdoptionAttempt::InFlight);
        }
        !blocked
    }

    /// Transport loss may be retried on the next event; a refusal waits for a change.
    pub(crate) fn finish_adoption(
        &mut self,
        peer_id: &str,
        outcome: &Result<(), AdmissionRefusal>,
    ) {
        match outcome {
            Ok(())
            | Err(AdmissionRefusal::PeerUnreachable)
            | Err(AdmissionRefusal::Offline)
            | Err(AdmissionRefusal::Cancelled) => {
                self.adoptions.remove(peer_id);
            }
            Err(_) => {
                let refused = AdoptionAttempt::Refused {
                    account_epoch: self.account.account_epoch,
                    advertised: self.advertised_memberships.get(peer_id).cloned(),
                };
                self.adoptions.insert(peer_id.to_string(), refused);
            }
        }
    }

    pub fn is_legacy_link(&self, peer_id: &str) -> bool {
        matches!(
            self.links.get(peer_id).map(|record| &record.admission),
            Some(LinkAdmission::Legacy { .. })
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_legacy_pair_for_test(&mut self, peer: crate::types::PairedPeer) {
        let id = peer.instance_id.clone();
        self.links.links.insert(
            id.clone(),
            LinkRecord {
                link_nonce: None,
                admission: LinkAdmission::Legacy {
                    public_key: peer.public_key.clone(),
                },
                remote_membership: None,
            },
        );
        self.paired_peers.insert(id, peer);
        self.bump_gate();
    }
}

pub fn blocked_message(reason: BlockReason) -> String {
    match reason {
        BlockReason::SignedOut => "Signed out — sign in to use the mesh".to_string(),
        BlockReason::PlanLimitLocked => {
            "plan_limit_locked: this machine is outside your plan's machine limit".to_string()
        }
        other => format!("Mesh link blocked: {other:?}"),
    }
}

/// Resolves once `peer_id`'s edge stops being open: on any gate change or at its signed expiry.
pub async fn wait_until_blocked(mesh: MeshState, peer_id: String) {
    loop {
        let (mut watch, deadline, now) = {
            let manager = mesh.lock();
            if !manager.traffic_open(&peer_id) {
                return;
            }
            (
                manager.gate_watch(),
                manager.admission_deadline(&peer_id),
                manager.now(),
            )
        };
        match deadline {
            Some(deadline) => {
                let remaining = std::time::Duration::from_secs(deadline.saturating_sub(now));
                tokio::select! {
                    changed = watch.changed() => if changed.is_err() { return },
                    _ = tokio::time::sleep(remaining) => {}
                }
            }
            None => {
                if watch.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}
