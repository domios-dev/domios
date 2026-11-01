use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

#[cfg(any(target_os = "macos", test))]
mod self_detection;
mod tailscale;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkIdentity {
    pub kind: String,
    pub network_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportState {
    NotInstalled,
    NotRunning,
    PermissionNeeded,
    SignedOut,
    Starting,
    Connected,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupAction {
    pub kind: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportStatus {
    pub display_name: String,
    pub state: TransportState,
    pub identity: Option<NetworkIdentity>,
    pub network_name: Option<String>,
    #[serde(skip_serializing)]
    pub user: Option<String>,
    pub addresses: Vec<IpAddr>,
    pub peer_listener: bool,
    pub encrypted: bool,
    pub setup_actions: Vec<SetupAction>,
}

pub trait NetworkTransportProvider: Send + Sync {
    fn detect(&self) -> TransportStatus;
}

pub fn active_provider() -> &'static dyn NetworkTransportProvider {
    &tailscale::Tailscale
}

pub fn listener_addresses(status: &TransportStatus, enabled: bool, port: u16) -> Vec<SocketAddr> {
    if !enabled || port == 0 || !status.peer_listener || status.state != TransportState::Connected {
        return Vec::new();
    }
    let mut addresses: Vec<_> = status
        .addresses
        .iter()
        .copied()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast())
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}
