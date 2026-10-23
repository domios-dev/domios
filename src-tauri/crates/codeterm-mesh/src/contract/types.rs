//! Mesh networking types — peer discovery, pairing, and communication.

use std::collections::HashMap;
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
    /// This peer's Ed25519 public key (base64). Shared with peers during pairing.
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

/// Drop addresses whose `ip` field doesn't parse as a real IpAddr.
///
/// Pre-v1.1.9 daemons could publish unparseable error-string IPs into
/// peer records when `tailscale ip -4` failed via the GUI bridge — see
/// commit f0ff692 / release/1.1.9 issue #28. The validate-before-cache
/// fix only protects fresh pairs; existing peers.json records and any
/// gossip from still-running v1.1.8 peers carry the poison forward.
///
/// Apply this on every inbound address path (load_peers on startup,
/// mesh_sync ingest, mesh_introduce, mesh_announce_inbound) so a single
/// poisoned peer can't keep re-poisoning the network.
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

/// Sent by a peer to tell us its current addresses changed (network swap,
/// new Tailscale lease, etc.). Receiver replaces the cached list for the
/// sender's instance_id wholesale — stale entries must die.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAnnounce {
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
    Pairing,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPeersResponse {
    pub paired: Vec<PairedPeer>,
    pub local_instance_name: String,
    #[serde(default)]
    pub local_last_active_ms: Option<u64>,
    #[serde(default)]
    pub local_dev: bool,
}

// ── Revocation ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RevocationList {
    /// Instance IDs whose mesh tokens have been revoked.
    pub revoked: Vec<String>,
    /// Instance IDs explicitly blocked from ever linking.
    pub blocked: Vec<String>,
    /// Locally-created tombstones. Their generation survives restart so stale
    /// gossip cannot recreate an explicitly removed peer.
    #[serde(default)]
    pub tombstones: HashMap<String, u64>,
    #[serde(default)]
    pub next_tombstone_generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_protocol_refuses_v1_and_accepts_v2_records() {
        assert_eq!(RELAY_PROTOCOL_VERSION, 2);
        assert!(!is_supported_relay_protocol(1));
        assert!(is_supported_relay_protocol(2));
        assert!(!is_supported_relay_protocol(0));
        assert!(!is_supported_relay_protocol(3));
    }

    #[test]
    fn tailscale_cgnat_recognized() {
        assert!(is_tailscale_ip("100.64.0.1"));
        assert!(is_tailscale_ip("100.65.0.10"));
        assert!(is_tailscale_ip("100.127.255.254"));
        assert!(!is_tailscale_ip("100.63.255.255"));
        assert!(!is_tailscale_ip("100.128.0.0"));
    }

    #[test]
    fn lan_private_ranges() {
        assert!(is_lan_private_ip("10.0.0.1"));
        assert!(is_lan_private_ip("172.16.0.1"));
        assert!(is_lan_private_ip("172.31.255.255"));
        assert!(is_lan_private_ip("192.168.1.1"));
        assert!(is_lan_private_ip("127.0.0.1"));
        assert!(!is_lan_private_ip("172.15.0.1"));
        assert!(!is_lan_private_ip("172.32.0.1"));
        assert!(!is_lan_private_ip("8.8.8.8"));
        assert!(!is_lan_private_ip("100.65.0.20")); // tailscale not LAN
    }

    #[test]
    fn address_tier_orders_tailscale_first_then_lan_then_public() {
        let ts = PeerAddress {
            ip: "100.65.0.30".into(),
            is_tailscale: true,
        };
        let lan = PeerAddress {
            ip: "192.168.1.2".into(),
            is_tailscale: false,
        };
        let pub_ip = PeerAddress {
            ip: "8.8.8.8".into(),
            is_tailscale: false,
        };
        assert!(address_tier(&ts) < address_tier(&lan));
        assert!(address_tier(&lan) < address_tier(&pub_ip));
    }

    #[test]
    fn address_tier_min_picks_tailscale() {
        let addrs = vec![
            PeerAddress {
                ip: "8.8.8.8".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "192.168.1.2".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "100.65.0.30".into(),
                is_tailscale: true,
            },
        ];
        let pick = addrs.iter().min_by_key(|a| address_tier(a)).unwrap();
        assert_eq!(pick.ip, "100.65.0.30");
    }

    #[test]
    fn sanitize_addresses_passes_clean_ips_through() {
        let addrs = vec![
            PeerAddress {
                ip: "100.65.0.30".into(),
                is_tailscale: true,
            },
            PeerAddress {
                ip: "192.168.1.2".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "::1".into(),
                is_tailscale: false,
            },
        ];
        let out = sanitize_addresses(&addrs);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn sanitize_addresses_drops_unparseable_strings() {
        // The actual poison strings observed in the wild from a broken `tailscale ip -4` GUI bridge: error text on stdout instead of an IP.
        let poisoned = vec![
            PeerAddress {
                ip: "Tailscale daemon is not running.".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "failed to connect to local backend".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "not-an-ip".into(),
                is_tailscale: false,
            },
        ];
        let out = sanitize_addresses(&poisoned);
        assert!(
            out.is_empty(),
            "all entries should be dropped, got: {:?}",
            out
        );
    }

    #[test]
    fn sanitize_addresses_preserves_clean_drops_poisoned_in_mixed_list() {
        let mixed = vec![
            PeerAddress {
                ip: "100.65.0.30".into(),
                is_tailscale: true,
            },
            PeerAddress {
                ip: "Tailscale daemon is not running.".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "192.168.1.2".into(),
                is_tailscale: false,
            },
            PeerAddress {
                ip: "garbage".into(),
                is_tailscale: false,
            },
        ];
        let out = sanitize_addresses(&mixed);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ip, "100.65.0.30");
        assert_eq!(out[1].ip, "192.168.1.2");
    }

    #[test]
    fn proactive_sync_skips_when_offline() {
        // Offline peer — never sync, regardless of last_sync state.
        assert!(!should_proactive_sync(false, false, 1000, None, 60));
        assert!(!should_proactive_sync(false, true, 1000, Some(0), 60));
        assert!(!should_proactive_sync(false, true, 10_000, Some(0), 60));
    }

    #[test]
    fn proactive_sync_fires_on_reconnect_regardless_of_last_sync() {
        // offline→online transition always triggers sync to refresh gossip immediately; previous last_sync timestamp is irrelevant.
        assert!(should_proactive_sync(true, false, 1000, None, 60));
        assert!(should_proactive_sync(true, false, 1000, Some(999), 60));
        assert!(should_proactive_sync(true, false, 1000, Some(0), 60));
    }

    #[test]
    fn proactive_sync_fires_when_last_sync_unknown_for_steady_online_peer() {
        // First iteration after a state where peer was already online but we have no record of having synced — must sync to populate the map.
        assert!(should_proactive_sync(true, true, 1000, None, 60));
    }

    #[test]
    fn proactive_sync_fires_when_interval_elapsed() {
        // Peer steady online and last_sync ≥ interval ago → sync to refresh gossip-discovered peers' last_seen before TTL eviction.
        assert!(should_proactive_sync(true, true, 1060, Some(1000), 60));
        assert!(should_proactive_sync(true, true, 1500, Some(1000), 60));
    }

    #[test]
    fn proactive_sync_skips_when_recently_synced() {
        // Peer steady online and last_sync < interval ago → skip to avoid hammering paired peers. Network traffic stays bounded.
        assert!(!should_proactive_sync(true, true, 1059, Some(1000), 60));
        assert!(!should_proactive_sync(true, true, 1001, Some(1000), 60));
        assert!(!should_proactive_sync(true, true, 1000, Some(1000), 60));
    }

    #[test]
    fn proactive_sync_handles_clock_skew_via_saturating_sub() {
        // behavior: don't blast sync requests after clock skew.
        assert!(!should_proactive_sync(true, true, 500, Some(1000), 60));
    }
}
