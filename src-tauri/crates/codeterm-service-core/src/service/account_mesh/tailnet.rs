use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::{extract::Request, middleware::Next};
use tokio::task::JoinSet;

use super::ServiceContext;
use crate::startup::rest_api::{listeners::serve_listener, RestApiControlState, RestApiPhase};

mod network_changes;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub struct PeerListener;

#[async_trait::async_trait]
pub(super) trait AddressChanges: Send {
    async fn selected(&self, signed_in: bool, enabled: bool, port: u16) -> Vec<SocketAddr>;
    async fn changed(&mut self) -> Result<(), ()>;
}

struct NativeAddresses(network_changes::Notifications);

#[async_trait::async_trait]
impl AddressChanges for NativeAddresses {
    async fn selected(&self, signed_in: bool, enabled: bool, port: u16) -> Vec<SocketAddr> {
        let interfaces = if signed_in && enabled {
            match tokio::task::spawn_blocking(if_addrs::get_if_addrs).await {
                Ok(Ok(interfaces)) => interfaces
                    .into_iter()
                    .map(|interface| interface.ip())
                    .collect(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        selected_addresses(signed_in, enabled, port, interfaces)
    }

    async fn changed(&mut self) -> Result<(), ()> {
        self.0.changed().await.map_err(|_| ())
    }
}

fn selected_addresses(
    signed_in: bool,
    enabled: bool,
    port: u16,
    interfaces: impl IntoIterator<Item = IpAddr>,
) -> Vec<SocketAddr> {
    if !signed_in || !enabled || port == 0 {
        return Vec::new();
    }
    let mut addresses: Vec<_> = interfaces
        .into_iter()
        .filter(|ip| codeterm_mesh::types::is_tailscale_ip(&ip.to_string()))
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

async fn mark_peer_listener(mut request: Request, next: Next) -> axum::response::Response {
    request.extensions_mut().insert(PeerListener);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        codeterm_mesh::reachability::PEER_ONLY_HEADER,
        axum::http::HeaderValue::from_static("1"),
    );
    response
}

struct Serving {
    tasks: JoinSet<SocketAddr>,
    sockets: BTreeMap<SocketAddr, tokio::task::AbortHandle>,
    failed: BTreeSet<SocketAddr>,
}

impl Serving {
    async fn account_changed(
        &mut self,
        previous: &codeterm_mesh::account_mesh::AccountMeshState,
        current: &codeterm_mesh::account_mesh::AccountMeshState,
    ) {
        if previous != current {
            self.failed.clear();
        }
        if previous.account_epoch != current.account_epoch || previous.presence != current.presence
        {
            self.withdraw(&[]).await;
        }
    }

    fn stopped(&mut self, result: Result<SocketAddr, tokio::task::JoinError>) {
        let address = match result {
            Ok(address) => Some(address),
            Err(error) => self
                .sockets
                .iter()
                .find_map(|(address, task)| (task.id() == error.id()).then_some(*address)),
        };
        if let Some(address) = address {
            self.sockets.remove(&address);
            self.failed.insert(address);
        }
    }

    async fn withdraw(&mut self, desired: &[SocketAddr]) {
        let mut stopping = BTreeSet::new();
        self.sockets.retain(|address, task| {
            if desired.contains(address) {
                true
            } else {
                task.abort();
                stopping.insert(task.id());
                false
            }
        });
        while !stopping.is_empty() {
            let Some(result) = self.tasks.join_next_with_id().await else {
                break;
            };
            let id = match &result {
                Ok((id, _)) => *id,
                Err(error) => error.id(),
            };
            if !stopping.remove(&id) {
                self.stopped(result.map(|(_, address)| address));
            }
        }
    }

    async fn reconcile(&mut self, desired: &[SocketAddr], router: &axum::Router) {
        self.withdraw(desired).await;
        while let Some(result) = self.tasks.try_join_next() {
            self.stopped(result);
        }
        for address in desired {
            if self.sockets.contains_key(address) {
                continue;
            }
            if self.failed.contains(address) {
                continue;
            }
            let listener = match codeterm_util::tcp_listener::bind(*address) {
                Ok(listener) => listener,
                Err(error) => {
                    log::warn!("[account-mesh] tailnet bind {address} failed: {error}");
                    self.failed.insert(*address);
                    continue;
                }
            };
            let router = router.clone();
            let address = *address;
            let task = self.tasks.spawn(async move {
                let (_stop, stopped) = tokio::sync::oneshot::channel();
                if let Err(error) = serve_listener(listener, router, stopped).await {
                    log::warn!("[account-mesh] tailnet listener {address} failed: {error}");
                }
                address
            });
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            match crate::startup::rest_api::prove_health(address, None, deadline).await {
                Ok(()) => {
                    self.sockets.insert(address, task);
                }
                Err(error) => {
                    task.abort();
                    self.failed.insert(address);
                    log::warn!("[account-mesh] tailnet listener not ready: {error}");
                }
            }
        }
    }
}

pub(super) async fn run(service: Arc<ServiceContext>, mesh: crate::MeshStateHandle) {
    let network = match network_changes::subscribe() {
        Ok(network) => NativeAddresses(network),
        Err(error) => {
            log::error!("[account-mesh] address notifications unavailable: {error}");
            return;
        }
    };
    run_with_network(service, mesh, network).await;
}

pub(super) async fn run_with_network(
    service: Arc<ServiceContext>,
    mesh: crate::MeshStateHandle,
    mut network: impl AddressChanges,
) {
    let Some(control) = service.try_ext::<RestApiControlState>().cloned() else {
        return;
    };
    let Some(router) = service.try_ext::<crate::ports::rest::LocalApiRouter>() else {
        return;
    };
    let router = router
        .0
        .clone()
        .layer(axum::middleware::from_fn(mark_peer_listener));
    let mut account = mesh.lock().gate_watch();
    let mut config = service.events.subscribe();
    let mut rest = control.subscribe_status();
    let mut serving = Serving {
        tasks: JoinSet::new(),
        sockets: BTreeMap::new(),
        failed: BTreeSet::new(),
    };
    let mut published = Vec::new();
    let mut last_account = mesh.lock().account_state().clone();
    loop {
        let current_account = mesh.lock().account_state().clone();
        let signed_in = current_account.signed_in_account().is_some();
        if last_account != current_account {
            serving
                .account_changed(&last_account, &current_account)
                .await;
            let retained: Vec<_> = serving.sockets.keys().copied().collect();
            if published != retained {
                control.listeners.publish_tailnet(retained.clone());
                published = retained;
            }
            last_account = current_account;
        }
        let enabled = {
            let config = service.ext::<crate::config::ConfigState>().lock();
            config.mesh.enabled && config.api.enabled
        };
        let port = control.bound_addr().map_or(0, |address| address.port());
        let listening = rest.borrow_and_update().phase == RestApiPhase::Listening;
        let desired = network
            .selected(signed_in, enabled && listening, port)
            .await;
        serving.withdraw(&desired).await;
        let retained: Vec<_> = serving.sockets.keys().copied().collect();
        if published != retained {
            control.listeners.publish_tailnet(retained.clone());
            published = retained;
        }
        serving.reconcile(&desired, &router).await;
        let ready = serving.sockets.keys().copied().collect::<Vec<_>>();
        if published != ready {
            control.listeners.publish_tailnet(ready.clone());
            published = ready;
        }
        tokio::select! {
            changed = network.changed() => {
                if changed.is_err() { break; }
                serving.failed.clear();
            },
            changed = account.changed() => if changed.is_err() { break; },
            changed = rest.changed() => {
                if changed.is_err() { break; }
                serving.failed.clear();
            },
            event = config_changed(&mut config) => {
                if event.is_err() { break; }
                serving.failed.clear();
            },
            Some(result) = serving.tasks.join_next(), if !serving.tasks.is_empty() => {
                serving.stopped(result);
                continue;
            }
        }
    }
    serving.withdraw(&[]).await;
    control.listeners.publish_tailnet(Vec::new());
}

async fn config_changed(
    receiver: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
) -> Result<(), ()> {
    loop {
        match receiver.recv().await {
            Ok(event) => match event.event {
                codeterm_events::SystemEvent::ConfigChanged { .. } => return Ok(()),
                codeterm_events::SystemEvent::Generic { event_type, .. }
                    if event_type == "config-changed" =>
                {
                    return Ok(())
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return Ok(()),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Err(()),
        }
    }
}
