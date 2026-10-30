//! Mesh networking types — account installations and authenticated communication.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

// ── Identity ────────────────────────────────────────────────────────

/// Persistent identity for this Domios instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshIdentity {
    pub instance_id: String,
    /// This peer's Ed25519 private key (base64). Used to sign mesh requests.
    #[serde(default)]
    pub private_key: Option<String>,
    /// This peer's Ed25519 public key (base64). Shared during the account handshake.
    #[serde(default)]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAddress {
    pub ip: String,
    pub is_tailscale: bool,
}

impl PeerAddress {
    pub fn from_ip(ip: &str) -> Self {
        Self {
            ip: ip.to_string(),
            is_tailscale: is_tailscale_ip(ip),
        }
    }
}

/// Tailscale CGNAT range: 100.64.0.0/10
pub fn is_tailscale_ip(ip: &str) -> bool {
    if let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() {
        let o = addr.octets();
        o[0] == 100 && (64..128).contains(&o[1])
    } else if let Ok(addr) = ip.parse::<std::net::Ipv6Addr>() {
        addr.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0]
    } else {
        false
    }
}

/// RFC1918 private LAN ranges: 10/8, 172.16/12, 192.168/16, plus loopback 127/8.
/// Tailscale CGNAT is intentionally NOT included here — it has its own tier.
pub fn is_lan_private_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let o = addr.octets();
    if o[0] == 10 {
        return true;
    }
    if o[0] == 192 && o[1] == 168 {
        return true;
    }
    if o[0] == 172 && (16..32).contains(&o[1]) {
        return true;
    }
    if o[0] == 127 {
        return true;
    }
    false
}

/// Address selection tier: lower = preferred. Tailscale is best (encrypted +
/// stable across networks), then LAN private (fast, no internet hop), then
/// anything else (treated as last-resort public).
pub fn address_tier(addr: &PeerAddress) -> u8 {
    if addr.is_tailscale {
        0
    } else if is_lan_private_ip(&addr.ip) {
        1
    } else {
        2
    }
}

/// Drop inbound addresses whose IP field is not a valid address.
pub fn sanitize_addresses(addresses: &[PeerAddress]) -> Vec<PeerAddress> {
    addresses
        .iter()
        .filter(|a| a.ip.parse::<std::net::IpAddr>().is_ok())
        .cloned()
        .collect()
}

/// Sync with a paired peer on every offline→online transition and periodically while it
/// stays online, so peer metadata (liveness, names) stays fresh.
pub fn should_proactive_sync(
    online: bool,
    was_online: bool,
    now_secs: u64,
    last_sync_secs: Option<u64>,
    interval_secs: u64,
) -> bool {
    if !online {
        return false;
    }
    if !was_online {
        return true;
    }
    match last_sync_secs {
        None => true,
        Some(t) => now_secs.saturating_sub(t) >= interval_secs,
    }
}

// ── Live address announce ───────────────────────────────────────────

pub const RELAY_PROTOCOL_VERSION: u16 = 2;
pub const MAX_SUPPORTED_RELAY_PROTOCOL_VERSION: u16 = 2;

pub const fn is_supported_relay_protocol(version: u16) -> bool {
    version >= RELAY_PROTOCOL_VERSION && version <= MAX_SUPPORTED_RELAY_PROTOCOL_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum WebUiEndpoint {
    TailscaleHttps { host: String, port: u16 },
    TailscaleHttp { ip: IpAddr, port: u16 },
    LanHttp { ip: IpAddr, port: u16 },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebUiAnnouncement {
    pub protocol_version: u16,
    pub endpoints: Vec<WebUiEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRevision {
    pub account_id: String,
    pub revision: u64,
    pub security_digest: String,
}

/// Sent by a peer to tell us its current addresses changed (network swap,
/// new Tailscale lease, etc.). Receiver replaces the cached list for the
/// sender's instance_id wholesale — stale entries must die.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAnnounce {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryRevision>,
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    #[serde(default = "default_true")]
    pub online: bool,
    #[serde(default)]
    pub web_ui: Option<WebUiAnnouncement>,
    #[serde(default)]
    pub relay_protocol_version: u16,
}

// ── Paired peers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedPeer {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    /// Peer's Ed25519 public key (base64) — used to verify all signed requests.
    pub public_key: String,
    /// ISO 8601 timestamp.
    pub paired_at: String,
    pub last_seen: Option<String>,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub access_level: AccessLevel,
    pub version: Option<String>,
    #[serde(default)]
    pub web_ui: Option<WebUiAnnouncement>,
    #[serde(default)]
    pub relay_protocol_version: u16,
    /// Runtime-only miss counter; not persisted to disk or sent over API.
    #[serde(skip)]
    pub consecutive_misses: u8,
    /// Learned from signed mesh gossip; not persisted to disk.
    #[serde(default)]
    pub last_active_ms: Option<u64>,
}

// ── Access levels ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    #[default]
    Full,
    ReadOnly,
    AgentIsolated,
    Custom(Vec<Permission>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ReadTabs,
    WriteTabs,
    SpawnAgents,
    ManageWorkspaces,
    ReadOutput,
    SendInput,
    ManageCrons,
    Admin,
}

// ── Gossip sync ─────────────────────────────────────────────────────

/// Peer info shared during gossip (no secrets — just addressing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipPeerInfo {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub dev: bool,
    #[serde(default)]
    pub last_active_ms: Option<u64>,
}

/// Mesh sync payload — exchanged between paired peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSyncPayload {
    /// Known peers in the mesh (addresses only, no tokens).
    pub peers: Vec<GossipPeerInfo>,
    /// Monotonic generation of the sender's authoritative peer-claim snapshot.
    #[serde(default)]
    pub generation: u64,
}

// ── Relay ───────────────────────────────────────────────────────────

/// Generic relay — proxy any API call to a mesh peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCommand {
    /// API method: GET, POST, PUT, DELETE, PATCH.
    #[serde(default = "default_method")]
    pub method: String,
    /// API path (e.g. "/api/tabs", "/api/agents/spawn").
    pub path: String,
    /// Request body (for POST/PUT/PATCH).
    #[serde(default)]
    pub body: serde_json::Value,
    /// Opaque per-logical-message delivery id (UUID), stable across the sender's
    /// transport retries. Lets the target dedupe by (authenticated sender, id).
    /// Absent for commands that opt out of at-most-once (reads, management).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// Relay response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResponse {
    pub ok: bool,
    pub status: u16,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

// ── Peer status ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    Online,
    Offline,
}

// ── API response types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatus {
    pub enabled: bool,
    pub instance_id: String,
    pub instance_name: String,
    pub build: String,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    pub paired_count: usize,
    pub transport: Option<crate::network_transport::TransportStatus>,
    pub web_ui: Option<WebUiAnnouncement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInstallation {
    pub installation_id: String,
    pub name: String,
    pub active: bool,
    pub signed_out: bool,
    pub membership_expires_at: Option<u64>,
    pub endpoint_expires_at: Option<u64>,
    pub instance_id: Option<String>,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPeersResponse {
    pub installations: Vec<AccountInstallation>,
    pub paired: Vec<PairedPeer>,
    pub local_instance_name: String,
    #[serde(default)]
    pub local_last_active_ms: Option<u64>,
    #[serde(default)]
    pub local_dev: bool,
}

#[cfg(test)]
mod tests;
