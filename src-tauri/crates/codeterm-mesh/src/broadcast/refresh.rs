use std::net::IpAddr;

use crate::network_transport::{self, TransportStatus};
use crate::types::WebUiAnnouncement;
use crate::MeshState;

#[derive(Default)]
pub(crate) struct RefreshState {
    running: bool,
    pending: bool,
    identity: Option<AddressIdentity>,
}

#[derive(Clone, PartialEq, Eq)]
struct AddressIdentity {
    addresses: Vec<IpAddr>,
    tailscale_ip: Option<IpAddr>,
    local_ip: Option<IpAddr>,
    candidates: Option<WebUiAnnouncement>,
    api_port: u16,
    build_hash: String,
    listeners: Vec<std::net::SocketAddr>,
}

fn listener_set(listeners: &[std::net::SocketAddr]) -> Vec<std::net::SocketAddr> {
    let mut listeners = listeners.to_vec();
    listeners.sort_unstable();
    listeners.dedup();
    listeners
}

struct ProbeInput {
    api_port: u16,
    build_hash: String,
    healthy: bool,
    verified: Option<WebUiAnnouncement>,
    listeners: Vec<std::net::SocketAddr>,
}

struct Observation {
    status: TransportStatus,
    identity: AddressIdentity,
    web_ui: Option<WebUiAnnouncement>,
}

impl Observation {
    fn collect(
        input: &ProbeInput,
        mut status: TransportStatus,
        local_ip: Option<String>,
        https: Option<&str>,
        mut verify: impl FnMut(&crate::types::WebUiEndpoint) -> bool,
    ) -> Self {
        network_transport::normalize_addresses(&mut status);
        let tailscale_ip = network_transport::preferred_ip(&status, input.api_port);
        let local_ip = local_ip
            .and_then(|value| value.parse::<IpAddr>().ok())
            .filter(|ip| {
                crate::types::is_lan_private_ip(&ip.to_string())
                    && crate::reachability::eligible(*ip)
            });
        let tailnet = tailscale_ip.map(|ip| ip.to_string());
        let lan = local_ip.map(|ip| ip.to_string());
        let https = (status.state == network_transport::TransportState::Connected)
            .then_some(https)
            .flatten();
        let candidates = crate::web_ui_endpoints::collect(
            tailnet.as_deref(),
            lan.as_deref(),
            input.api_port,
            https,
        );
        let web_ui = input
            .healthy
            .then(|| {
                crate::web_ui_endpoints::collect_verified_with(
                    tailnet.as_deref(),
                    lan.as_deref(),
                    input.api_port,
                    https,
                    |endpoint| {
                        input
                            .verified
                            .as_ref()
                            .is_some_and(|known| known.endpoints.contains(endpoint))
                            || verify(endpoint)
                    },
                )
            })
            .flatten();
        let addresses = status.addresses.clone();
        Self {
            status,
            identity: AddressIdentity {
                addresses,
                tailscale_ip,
                local_ip,
                candidates,
                api_port: input.api_port,
                build_hash: input.build_hash.clone(),
                listeners: input.listeners.clone(),
            },
            web_ui,
        }
    }
}

struct RefreshOwner<'a> {
    mesh: &'a MeshState,
    active: bool,
}

impl Drop for RefreshOwner<'_> {
    fn drop(&mut self) {
        if self.active {
            self.mesh.lock().address_refresh.running = false;
        }
    }
}

pub(super) fn refresh(mesh: &MeshState, force: bool) -> bool {
    refresh_with(mesh, force, |input| {
        let status = network_transport::active_provider().detect();
        let local_ip = crate::discovery::local_ip_address().ok();
        let https = crate::serve_setup::configured_https_url(input.api_port);
        Observation::collect(input, status, local_ip, https.as_deref(), |endpoint| {
            crate::serve_setup::endpoint_matches_build(
                &crate::web_ui_endpoints::endpoint_base_url(endpoint),
                &input.build_hash,
            )
        })
    })
}

fn refresh_with(
    mesh: &MeshState,
    force: bool,
    mut probe: impl FnMut(&ProbeInput) -> Observation,
) -> bool {
    {
        let mut manager = mesh.lock();
        if manager.address_refresh.running {
            manager.address_refresh.pending |= force;
            return false;
        }
        manager.address_refresh.running = true;
    }
    let mut owner = RefreshOwner { mesh, active: true };
    let mut changed = false;
    let mut reverify = force;
    loop {
        let input = {
            let mut manager = mesh.lock();
            manager.address_refresh.pending = false;
            let same_build = manager
                .address_refresh
                .identity
                .as_ref()
                .is_some_and(|known| {
                    known.api_port == manager.api_port
                        && known.build_hash == manager.build_hash
                        && known.listeners == listener_set(&manager.ready_listeners)
                });
            ProbeInput {
                api_port: manager.api_port,
                build_hash: manager.build_hash.clone(),
                healthy: manager.local_http_healthy,
                verified: (same_build && !reverify)
                    .then(|| manager.local_web_ui.clone())
                    .flatten(),
                listeners: listener_set(&manager.ready_listeners),
            }
        };
        let observation = probe(&input);
        let mut manager = mesh.lock();
        if manager.api_port != input.api_port
            || manager.build_hash != input.build_hash
            || manager.local_http_healthy != input.healthy
            || listener_set(&manager.ready_listeners) != input.listeners
        {
            manager.address_refresh.pending = true;
        } else {
            let address_changed =
                manager.address_refresh.identity.as_ref() != Some(&observation.identity);
            let capability_changed = manager.local_web_ui != observation.web_ui;
            let transport_changed = manager.set_network_transport(observation.status);
            manager.local_ip = observation.identity.local_ip.map(|ip| ip.to_string());
            manager.local_web_ui = observation.web_ui;
            manager.address_refresh.identity = Some(observation.identity);
            if (address_changed || capability_changed) && !transport_changed {
                manager.bump_gate();
            }
            changed |= address_changed || capability_changed;
            if address_changed {
                log::debug!("[mesh/refresh] local endpoint identity changed");
            }
        }
        if !manager.address_refresh.pending {
            manager.address_refresh.running = false;
            owner.active = false;
            return changed;
        }
        reverify = true;
    }
}

#[cfg(test)]
mod tests;
