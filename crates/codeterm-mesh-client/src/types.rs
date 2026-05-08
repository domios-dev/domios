//! Mesh types — shared between relay and full peers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Persistent identity for a mesh participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshIdentity {
    pub instance_id: String,
    /// Ed25519 private key (base64). Used to sign mesh requests.
    #[serde(default)]
    pub private_key: Option<String>,
    /// Ed25519 public key (base64). Shared with peers during pairing.
    #[serde(default)]
    pub public_key: Option<String>,
}

/// Peer address with Tailscale detection.
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
    } else {
        false
    }
}

/// RFC1918 private LAN ranges: 10/8, 172.16/12, 192.168/16, plus loopback 127/8.
pub fn is_lan_private_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else { return false; };
    let o = addr.octets();
    if o[0] == 10 { return true; }
    if o[0] == 192 && o[1] == 168 { return true; }
    if o[0] == 172 && (16..32).contains(&o[1]) { return true; }
    if o[0] == 127 { return true; }
    false
}

/// Address selection tier: lower = preferred.
pub fn address_tier(addr: &PeerAddress) -> u8 {
    if addr.is_tailscale { 0 }
    else if is_lan_private_ip(&addr.ip) { 1 }
    else { 2 }
}

/// Filter out addresses with unparseable IPs.
pub fn sanitize_addresses(addresses: &[PeerAddress]) -> Vec<PeerAddress> {
    addresses
        .iter()
        .filter(|a| a.ip.parse::<std::net::IpAddr>().is_ok())
        .cloned()
        .collect()
}

/// Paired peer info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedPeer {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub public_key: String,
    pub paired_at: String,
    pub last_seen: Option<String>,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub access_level: AccessLevel,
    pub version: Option<String>,
}

/// Access levels for mesh participants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    Full,
    ReadOnly,
    AgentIsolated,
    Custom(Vec<Permission>),
}

impl Default for AccessLevel {
    fn default() -> Self {
        Self::Full
    }
}

/// Individual permissions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ReadPanes,
    WritePanes,
    SpawnAgents,
    ManageWorkspaces,
    ReadOutput,
    SendInput,
    ManageCrons,
    Admin,
}

/// Relay capabilities (restricted set for relay participants).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RelayCapability {
    /// Can read pane output
    PaneRead,
    /// Can send text to panes
    PaneSendText,
    /// Can send notifications
    Notify,
}

impl RelayCapability {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelayCapability::PaneRead => "pane_read",
            RelayCapability::PaneSendText => "pane_send_text",
            RelayCapability::Notify => "notify",
        }
    }
}

/// Pairing request from a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    pub from_address: String,
    pub from_port: u16,
    pub public_key: String,
    pub timestamp: u64,
    #[serde(default = "default_pairing_status")]
    pub status: PairingStatus,
}

fn default_pairing_status() -> PairingStatus {
    PairingStatus::Pending
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PairingStatus {
    Pending,
    Accepted,
    Rejected,
    Expired,
}

/// Accept response during pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAccept {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    pub public_key: String,
}

/// Peer info shared during gossip sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipPeerInfo {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub dev: bool,
}

/// Mesh sync payload — exchanged between paired peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSyncPayload {
    pub peers: Vec<GossipPeerInfo>,
}

/// Generic relay command — proxy any API call to a mesh peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCommand {
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: serde_json::Value,
}

fn default_method() -> String { "GET".to_string() }

/// Relay response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResponse {
    pub ok: bool,
    pub status: u16,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Peer introduction — sent by a paired mediator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerIntroduction {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub public_key: String,
    pub introduced_by: String,
}

/// Address announce from a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAnnounce {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
}

/// Peer status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    Online,
    Offline,
    Pairing,
}

/// Discovered peer (not yet paired).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub discovery_method: DiscoveryMethod,
    pub last_seen: u64,
    #[serde(default)]
    pub dev: bool,
    #[serde(default)]
    pub mesh_id: Option<String>,
    #[serde(default)]
    pub vouched_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    Mdns,
    Tailscale,
    Udp,
    Manual,
    Gossip,
}

/// Mesh status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatus {
    pub enabled: bool,
    pub instance_id: String,
    pub instance_name: String,
    pub build: String,
    pub discovery_active: bool,
    pub broadcast_active: bool,
    pub broadcast_expires_at: Option<u64>,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    pub paired_count: usize,
    pub discovered_count: usize,
}

/// List of paired and discovered peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPeersResponse {
    pub paired: Vec<PairedPeer>,
    pub discovered: Vec<DiscoveredPeer>,
    pub incoming_requests: Vec<PairingRequest>,
}

/// Revocation list for mesh security.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RevocationList {
    pub revoked: Vec<String>,
    pub blocked: Vec<String>,
}

// ── Pane API types (for relay) ─────────────────────────────────────

/// Information about a pane on a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub title: Option<String>,
    pub agent_type: Option<String>,
}

/// Request to send text to a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextRequest {
    pub text: Option<String>,
    #[serde(default = "default_true")]
    pub enter: bool,
}

/// Response from send text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResponse {
    pub ok: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub echo_observed: bool,
    #[serde(default)]
    pub submit_observed: bool,
}

/// Pane output response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneOutputResponse {
    pub output: String,
    #[serde(default)]
    pub offset: usize,
}

fn default_true() -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailscale_cgnat_recognized() {
        assert!(is_tailscale_ip("100.64.0.1"));
        assert!(is_tailscale_ip("100.84.159.37"));
        assert!(is_tailscale_ip("100.127.255.254"));
        assert!(!is_tailscale_ip("100.63.255.255"));
        assert!(!is_tailscale_ip("100.128.0.0"));
    }

    #[test]
    fn lan_private_ranges() {
        assert!(is_lan_private_ip("10.0.0.1"));
        assert!(is_lan_private_ip("172.16.0.1"));
        assert!(is_lan_private_ip("192.168.1.1"));
        assert!(is_lan_private_ip("127.0.0.1"));
        assert!(!is_lan_private_ip("172.15.0.1"));
        assert!(!is_lan_private_ip("172.32.0.1"));
        assert!(!is_lan_private_ip("8.8.8.8"));
        assert!(!is_lan_private_ip("100.84.0.1")); // tailscale not LAN
    }

    #[test]
    fn address_tier_orders_tailscale_first() {
        let ts = PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true };
        let lan = PeerAddress { ip: "192.168.1.2".into(), is_tailscale: false };
        assert!(address_tier(&ts) < address_tier(&lan));
    }

    #[test]
    fn sanitize_addresses_drops_invalid() {
        let addrs = vec![
            PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true },
            PeerAddress { ip: "not-an-ip".into(), is_tailscale: false },
        ];
        let out = sanitize_addresses(&addrs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ip, "100.84.1.2");
    }
}