use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{extract::Request, middleware::Next};
use tokio::task::JoinSet;

use super::ServiceContext;
use crate::startup::rest_api::{listeners::serve_listener, RestApiControlState, RestApiPhase};

mod network_changes;
mod retry;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub struct TailnetListener;

#[async_trait::async_trait]
pub(super) trait AddressChanges: Send {
    async fn selected(&self, enabled: bool, port: u16) -> Vec<SocketAddr>;
    async fn changed(&mut self) -> Result<(), ()>;
}

struct NativeAddresses(
    network_changes::Notifications,
    crate::MeshStateHandle,
    Arc<ServiceContext>,
);

#[async_trait::async_trait]
impl AddressChanges for NativeAddresses {
    async fn selected(&self, enabled: bool, port: u16) -> Vec<SocketAddr> {
        let status = tokio::task::spawn_blocking(|| {
            codeterm_mesh::network_transport::active_provider().detect()
        })
        .await
        .ok();
        status.map_or_else(Vec::new, |status| {
            let addresses =
                codeterm_mesh::network_transport::listener_addresses(&status, enabled, port);
            let changed = self.1.lock().set_network_transport(status);
            if changed {
                self.2.ui.emit("mesh-state-changed", serde_json::json!({}));
            }
            addresses
        })
    }

    async fn changed(&mut self) -> Result<(), ()> {
        self.0.changed().await.map_err(|_| ())?;
        let service = self.2.clone();
        tokio::spawn(async move {
            let _ = service.retry_account_sign_out(None).await;
        });
        Ok(())
    }
}

#[cfg(test)]
fn selected_addresses(
    enabled: bool,
    port: u16,
    interfaces: impl IntoIterator<Item = IpAddr>,
) -> Vec<SocketAddr> {
    if !enabled || port == 0 {
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

async fn mark_tailnet_listener(mut request: Request, next: Next) -> axum::response::Response {
    request.extensions_mut().insert(TailnetListener);
    next.run(request).await
}

struct Serving {
    tasks: JoinSet<SocketAddr>,
    sockets: BTreeMap<SocketAddr, tokio::task::AbortHandle>,
    failed: BTreeMap<SocketAddr, retry::Failure>,
    reused: BTreeSet<SocketAddr>,
}

impl Serving {
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
            self.failed.entry(address).or_default().record(
                tokio::time::Instant::now(),
                format!("listener {address} exited"),
            );
        }
    }

    async fn withdraw(&mut self, desired: &[SocketAddr]) {
        self.reused.retain(|address| desired.contains(address));
        self.failed.retain(|address, _| desired.contains(address));
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

    fn ready(&self) -> Vec<SocketAddr> {
        self.sockets
            .keys()
            .chain(self.reused.iter())
            .copied()
            .collect()
    }

    fn retry_now(&mut self) {
        for failure in self.failed.values_mut() {
            failure.reset_deadline();
        }
    }

    async fn reconcile(
        &mut self,
        desired: &[SocketAddr],
        router: &axum::Router,
        primary: &[SocketAddr],
    ) {
        self.reconcile_with_bind(desired, router, primary, codeterm_util::tcp_listener::bind)
            .await;
    }

    async fn reconcile_with_bind(
        &mut self,
        desired: &[SocketAddr],
        router: &axum::Router,
        primary: &[SocketAddr],
        mut bind: impl FnMut(SocketAddr) -> std::io::Result<std::net::TcpListener>,
    ) {
        self.reused
            .retain(|address| primary.iter().any(|bind| covers(*bind, *address)));
        self.withdraw(desired).await;
        while let Some(result) = self.tasks.try_join_next() {
            self.stopped(result);
        }
        for address in desired {
            if self.sockets.contains_key(address) || self.reused.contains(address) {
                continue;
            }
            if self
                .failed
                .get(address)
                .is_some_and(|failure| !failure.due(tokio::time::Instant::now()))
            {
                continue;
            }
            let listener = match bind(*address) {
                Ok(listener) => listener,
                Err(error) if reusable_primary(error.kind(), primary, *address) => {
                    self.reused.insert(*address);
                    self.failed.remove(address);
                    continue;
                }
                Err(error) => {
                    self.failed.entry(*address).or_default().record(
                        tokio::time::Instant::now(),
                        format!("bind {address}: {}", error.kind()),
                    );
                    continue;
                }
            };
            let router = router.clone();
            let address = *address;
            let task = self.tasks.spawn(async move {
                let (_stop, stopped) = tokio::sync::oneshot::channel();
                if let Err(error) = serve_listener(listener, router, stopped).await {
                    log::debug!("[account-mesh] tailnet listener {address} failed: {error}");
                }
                address
            });
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            match crate::startup::rest_api::prove_health(address, None, deadline).await {
                Ok(()) => {
                    self.failed.remove(&address);
                    self.sockets.insert(address, task);
                }
                Err(error) => {
                    task.abort();
                    self.failed
                        .entry(address)
                        .or_default()
                        .record(tokio::time::Instant::now(), error);
                }
            }
        }
    }
}

pub(super) async fn run(service: Arc<ServiceContext>, mesh: crate::MeshStateHandle) {
    let network = match network_changes::subscribe() {
        Ok(network) => NativeAddresses(network, mesh.clone(), service.clone()),
        Err(error) => {
            log::error!("[account-mesh] address notifications unavailable: {error}");
            return;
        }
    };
    run_with_network(service, network).await;
}

pub(super) async fn run_with_network(
    service: Arc<ServiceContext>,
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
        .layer(axum::middleware::from_fn(mark_tailnet_listener));
    let mut config = service.events.subscribe();
    let mut rest = control.subscribe_status();
    let mut serving = Serving {
        tasks: JoinSet::new(),
        sockets: BTreeMap::new(),
        failed: BTreeMap::new(),
        reused: BTreeSet::new(),
    };
    let mut published = Vec::new();
    loop {
        let enabled = {
            let config = service.ext::<crate::config::ConfigState>().lock();
            config.api.enabled && config.mesh.enabled
        };
        let port = control.bound_addr().map_or(0, |address| address.port());
        let listening = rest.borrow_and_update().phase == RestApiPhase::Listening;
        let desired = if enabled && listening {
            network.selected(true, port).await
        } else {
            Vec::new()
        };
        let primary = if listening {
            control.listeners.primary_ready()
        } else {
            Vec::new()
        };
        serving
            .reused
            .retain(|address| primary.iter().any(|bind| covers(*bind, *address)));
        serving.withdraw(&desired).await;
        let retained = serving.ready();
        if published != retained {
            control.listeners.publish_tailnet(retained.clone());
            published = retained;
            service.ui.emit("mesh-state-changed", serde_json::json!({}));
        }
        serving.reconcile(&desired, &router, &primary).await;
        let ready = serving.ready();
        if published != ready {
            control.listeners.publish_tailnet(ready.clone());
            published = ready;
            service.ui.emit("mesh-state-changed", serde_json::json!({}));
        }
        let retry_at = serving
            .failed
            .values()
            .filter_map(|failure| failure.retry_at())
            .min();
        tokio::select! {
            _ = async {
                match retry_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending::<()>().await,
                }
            } => {},
            changed = network.changed() => {
                if changed.is_err() { break; }
                serving.retry_now();
            },
            changed = rest.changed() => {
                if changed.is_err() { break; }
                serving.retry_now();
            },
            event = config_changed(&mut config) => {
                if event.is_err() { break; }
                serving.retry_now();
            },
            Some(result) = serving.tasks.join_next(), if !serving.tasks.is_empty() => {
                serving.stopped(result);
                continue;
            }
        }
    }
    serving.withdraw(&[]).await;
    control.listeners.publish_tailnet(Vec::new());
    service.ui.emit("mesh-state-changed", serde_json::json!({}));
}

fn reusable_primary(
    error: std::io::ErrorKind,
    primary: &[SocketAddr],
    address: SocketAddr,
) -> bool {
    error == std::io::ErrorKind::AddrInUse && primary.iter().any(|bind| covers(*bind, address))
}

fn covers(bind: SocketAddr, address: SocketAddr) -> bool {
    bind.port() == address.port()
        && (bind.ip() == address.ip()
            || (bind.ip().is_unspecified() && bind.is_ipv4() == address.is_ipv4()))
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
