use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use codeterm_account::account_mesh::wire::{
    Eligibility, MeshEndpoint, MeshManagementRow, RegistryPresence,
};
use codeterm_mesh::account_mesh::link::{
    connect_target_with, prefers_initiating, HttpLinkTransport,
};
use codeterm_mesh::account_mesh::{AdmissionRefusal, LinkTarget};

use super::{AccountMeshService, ServiceContext, ACCOUNT_MESH_CHANGED};

pub(super) const CONNECTIVITY_CHANGED: &str = "mesh-connectivity-changed";
const RETRY_INTERVAL: u64 = 30;

#[derive(Default)]
struct Retry {
    failures: u32,
    retry_at: Option<u64>,
    transport_failure: bool,
}

impl Retry {
    fn failed(&mut self, now: u64, refusal: &AdmissionRefusal) {
        self.failures = self.failures.saturating_add(1);
        self.transport_failure = matches!(
            refusal,
            AdmissionRefusal::Offline | AdmissionRefusal::PeerUnreachable
        );
        let delay = match refusal {
            AdmissionRefusal::RateLimited {
                retry_after_seconds,
            } => *retry_after_seconds,
            AdmissionRefusal::Offline | AdmissionRefusal::PeerUnreachable => RETRY_INTERVAL
                .saturating_mul(1 << self.failures.saturating_sub(1).min(4))
                .min(300),
            _ => {
                self.retry_at = None;
                return;
            }
        };
        self.retry_at = Some(now.saturating_add(delay.max(1)));
    }
}

#[derive(Default)]
struct Reconciliation {
    epoch: Option<u64>,
    observations: BTreeMap<String, MeshManagementRow>,
    retries: BTreeMap<String, Retry>,
    fallback_at: BTreeMap<String, u64>,
    server_retry_at: u64,
    registry_retry: Option<Retry>,
}

impl Reconciliation {
    fn enter_epoch(&mut self, epoch: u64) {
        if self.epoch != Some(epoch) {
            *self = Self {
                epoch: Some(epoch),
                ..Self::default()
            };
        }
    }

    fn observe(&mut self, epoch: u64, rows: &[MeshManagementRow]) {
        self.enter_epoch(epoch);
        self.retries
            .retain(|id, _| rows.iter().any(|row| &row.installation_id == id));
        self.fallback_at
            .retain(|id, _| rows.iter().any(|row| &row.installation_id == id));
        for row in rows {
            if self.observations.get(&row.installation_id) != Some(row) {
                self.retries.remove(&row.installation_id);
                self.fallback_at.remove(&row.installation_id);
            }
        }
        self.observations = rows
            .iter()
            .map(|row| (row.installation_id.clone(), row.clone()))
            .collect();
    }

    fn due(&self, installation: &str, now: u64) -> bool {
        now >= self.server_retry_at
            && self
                .retries
                .get(installation)
                .is_none_or(|retry| retry.retry_at.is_some_and(|at| at <= now))
    }

    fn dial_due(&mut self, own: &str, installation: &str, now: u64) -> bool {
        if !prefers_initiating(own, installation) && !self.retries.contains_key(installation) {
            let at = self
                .fallback_at
                .entry(installation.to_owned())
                .or_insert_with(|| now.saturating_add(RETRY_INTERVAL));
            if now < *at {
                return false;
            }
        }
        let due = self.due(installation, now);
        if due {
            self.fallback_at.remove(installation);
        }
        due
    }

    fn clear_peer(&mut self, installation: &str) {
        self.retries.remove(installation);
        self.fallback_at.remove(installation);
    }

    fn network_changed(&mut self) {
        self.retries.retain(|_, retry| !retry.transport_failure);
        self.fallback_at.clear();
    }

    fn registry_refresh_due(&self, now: u64) -> bool {
        self.registry_retry
            .as_ref()
            .and_then(|retry| retry.retry_at)
            .is_some_and(|at| at <= now)
    }

    fn deadline(&self, now: u64, peers_ready: bool) -> Option<u64> {
        if let Some(retry) = &self.registry_retry {
            return retry.retry_at;
        }
        if !peers_ready {
            return None;
        }
        self.retries
            .values()
            .filter_map(|retry| retry.retry_at)
            .chain(
                self.fallback_at
                    .iter()
                    .filter(|(id, _)| !self.retries.contains_key(*id))
                    .map(|(_, at)| *at),
            )
            .chain((self.server_retry_at > now).then_some(self.server_retry_at))
            .map(|at| at.max(self.server_retry_at))
            .min()
    }
}

#[derive(Default)]
struct Publication {
    pairs: Vec<codeterm_mesh::account_mesh::protocol::AccountMeshPair>,
    registry: Option<codeterm_account::account_mesh::RegistryObservation>,
}

impl Publication {
    fn update(
        &mut self,
        pairs: Vec<codeterm_mesh::account_mesh::protocol::AccountMeshPair>,
        registry: Option<codeterm_account::account_mesh::RegistryObservation>,
        mut emit: impl FnMut(&'static str),
    ) {
        let mut pairs = pairs;
        pairs.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        let registry = registry.map(|mut value| {
            value.observed_at = 0;
            value
                .rows
                .sort_by(|a, b| a.installation_id.cmp(&b.installation_id));
            value
        });
        if self.pairs == pairs && self.registry == registry {
            return;
        }
        self.pairs = pairs;
        self.registry = registry;
        emit(ACCOUNT_MESH_CHANGED);
        emit("mesh-state-changed");
    }
}

fn target(row: &MeshManagementRow, own: &str, now: u64) -> Option<LinkTarget> {
    if row.installation_id == own
        || !row.active
        || row.eligibility != Eligibility::Eligible
        || row.presence != RegistryPresence::Present
        || row.membership_expires_at.is_none_or(|at| at <= now)
    {
        return None;
    }
    let endpoint = row
        .endpoint
        .as_ref()
        .filter(|endpoint| endpoint.expires_at > now)?;
    let endpoint = MeshEndpoint::parse(&endpoint.tailnet_ip, endpoint.api_port)?;
    Some(LinkTarget {
        installation_id: row.installation_id.clone(),
        ip: endpoint.ip().to_string(),
        api_port: endpoint.port(),
    })
}

pub(super) async fn run(
    service: Arc<ServiceContext>,
    account: Arc<AccountMeshService>,
    mesh: crate::MeshStateHandle,
) {
    let mut changes = account.mesh.subscribe();
    let mut connections = mesh.lock().gate_watch();
    let mut network = service.events.subscribe();
    let mut publication = Publication::default();
    let mut reconciliation = Reconciliation::default();
    let mut membership = None;
    let mut refresh = false;
    loop {
        connections.borrow_and_update();
        let enabled = service.configuration().mesh.enabled;
        let state = changes.borrow_and_update().clone();
        let now = mesh.lock().now();
        reconciliation.enter_epoch(state.account_epoch);
        let current = state
            .current_membership(now)
            .filter(|_| enabled)
            .map(|membership| membership.token.clone());
        if current.is_some() && (refresh || current != membership) {
            membership = current;
            refresh = false;
            match account.mesh.list_fresh().await {
                Ok(_) => {
                    reconciliation.registry_retry = None;
                }
                Err(refusal) => {
                    let refusal = super::authority::admission_refusal(refusal);
                    reconciliation
                        .registry_retry
                        .get_or_insert_with(Retry::default)
                        .failed(now, &refusal);
                    log::debug!("[mesh/account] Registry reconciliation deferred: {refusal:?}");
                }
            }
        } else {
            membership = current;
        }
        let state = account.mesh.state();
        if state.registry_failure.is_none() {
            reconciliation.registry_retry = None;
        }
        let mut peers_ready = false;
        if let Some(own) = state.current_membership(now).filter(|_| {
            enabled
                && reconciliation.registry_retry.is_none()
                && mesh.lock().account_state() == &super::mesh_state(&state)
        }) {
            if let Some(registry) = state
                .registry
                .as_ref()
                .filter(|registry| registry.cursor.is_none())
            {
                peers_ready = true;
                reconciliation.observe(state.account_epoch, &registry.rows);
                let active: BTreeSet<_> = registry
                    .rows
                    .iter()
                    .filter(|row| row.active && row.eligibility == Eligibility::Eligible)
                    .map(|row| row.installation_id.as_str())
                    .collect();
                let removed: Vec<_> = mesh
                    .lock()
                    .account_mesh_pairs()
                    .into_iter()
                    .filter(|peer| {
                        peer.installation_id
                            .as_deref()
                            .is_none_or(|id| !active.contains(id))
                    })
                    .map(|peer| peer.instance_id)
                    .collect();
                for id in removed {
                    mesh.lock().remove_account_peer(&id);
                }
                for row in &registry.rows {
                    if account.mesh.state().account_epoch != state.account_epoch
                        || !service.configuration().mesh.enabled
                    {
                        break;
                    }
                    let now = mesh.lock().now();
                    let Some(target) = target(row, &own.verified.installation_id, now) else {
                        reconciliation.clear_peer(&row.installation_id);
                        continue;
                    };
                    let connected = {
                        let manager = mesh.lock();
                        manager.account_mesh_pairs().iter().any(|peer| {
                            peer.installation_id.as_deref() == Some(row.installation_id.as_str())
                                && peer.online
                                && manager.traffic_open(&peer.instance_id)
                        })
                    };
                    if connected {
                        reconciliation.clear_peer(&row.installation_id);
                        continue;
                    }
                    if !reconciliation.dial_due(
                        &own.verified.installation_id,
                        &row.installation_id,
                        now,
                    ) {
                        continue;
                    }
                    match connect_target_with(&mesh, &HttpLinkTransport, target).await {
                        Ok(_) => {
                            reconciliation.clear_peer(&row.installation_id);
                        }
                        Err(refusal) => {
                            reconciliation
                                .retries
                                .entry(row.installation_id.clone())
                                .or_default()
                                .failed(now, &refusal);
                            log::debug!(
                                "[mesh/account] Connection to {} deferred: {}",
                                row.installation_id,
                                refusal
                            );
                            if let AdmissionRefusal::RateLimited {
                                retry_after_seconds,
                            } = refusal
                            {
                                reconciliation.server_retry_at =
                                    now.saturating_add(retry_after_seconds.max(1));
                                break;
                            }
                        }
                    }
                }
            }
        } else if !enabled || state.current_membership(now).is_none() {
            reconciliation = Reconciliation::default();
        }
        let pairs = mesh.lock().account_mesh_pairs();
        let registry = account.mesh.state().registry.clone();
        publication.update(pairs, registry, |event| {
            service.ui.emit(event, serde_json::Value::Null);
        });
        let deadline = reconciliation.deadline(mesh.lock().now(), peers_ready);
        tokio::select! {
            changed = changes.changed() => { if changed.is_err() { return; } },
            changed = connections.changed() => { if changed.is_err() { return; } },
            changed = connectivity_changed(&mut network) => {
                if changed.is_err() { return; }
                reconciliation.network_changed();
            },
            _ = async {
                match deadline {
                    Some(at) => {
                        let delay = Duration::from_secs(at.saturating_sub(mesh.lock().now()));
                        tokio::time::sleep(delay).await;
                    },
                    None => std::future::pending::<()>().await,
                }
            } => { refresh = reconciliation.registry_refresh_due(mesh.lock().now()); }
        }
    }
}

async fn connectivity_changed(
    events: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
) -> Result<(), ()> {
    loop {
        match events.recv().await {
            Ok(event) if matches!(event.event, codeterm_events::SystemEvent::Generic { ref event_type, .. } if event_type == CONNECTIVITY_CHANGED) => {
                return Ok(())
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return Ok(()),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Err(()),
        }
    }
}

#[cfg(test)]
mod tests;
