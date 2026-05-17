//! Mesh networking types — peer discovery, pairing, and communication.

use serde::{Deserialize, Serialize};

// ── Identity ────────────────────────────────────────────────────────

/// Persistent identity for this CodeTerm instance.
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

// ── Discovery ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    Mdns,
    Tailscale,
    Udp,
    Manual,
    /// Learned about this peer via a gossip-sync push from another paired
    /// peer (mesh_sync handler). Distinct from Mdns/Udp because the local
    /// daemon never directly observed the announcement — diagnostically
    /// important when tracking down poisoned-address propagation.
    Gossip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAddress {
    pub ip: String,
    pub is_tailscale: bool,
}

impl PeerAddress {
    pub fn from_ip(ip: &str) -> Self {
        Self { ip: ip.to_string(), is_tailscale: is_tailscale_ip(ip) }
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
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else { return false; };
    let o = addr.octets();
    if o[0] == 10 { return true; }
    if o[0] == 192 && o[1] == 168 { return true; }
    if o[0] == 172 && (16..32).contains(&o[1]) { return true; }
    if o[0] == 127 { return true; }
    false
}

/// Address selection tier: lower = preferred. Tailscale is best (encrypted +
/// stable across networks), then LAN private (fast, no internet hop), then
/// anything else (treated as last-resort public).
pub fn address_tier(addr: &PeerAddress) -> u8 {
    if addr.is_tailscale { 0 }
    else if is_lan_private_ip(&addr.ip) { 1 }
    else { 2 }
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

/// Decide whether the periodic background loop should send_sync to a paired
/// peer this iteration. We sync on every offline→online transition (so
/// reconnects refresh the gossip view immediately) AND periodically while
/// the peer stays online, so that gossip-discovered peers known only via
/// this paired peer don't decay past PEER_TTL_SECS in cleanup_stale_peers.
///
/// Without periodic sync, a discovered peer's last_seen is only refreshed
/// when its mediator pushes gossip to us (mesh_sync inbound), the cadence
/// of which we don't control. Observed in the wild: paired peers sync at
/// ~5–10 min intervals, but PEER_TTL_SECS = 300 — so any gossip-discovered
/// peer falls out of `mesh peers` between pushes.
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

/// Sent by a peer to tell us its current addresses changed (network swap,
/// new Tailscale lease, etc.). Receiver replaces the cached list for the
/// sender's instance_id wholesale — stale entries must die.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAnnounce {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    pub discovery_method: DiscoveryMethod,
    /// Unix timestamp (seconds) when last seen via discovery.
    pub last_seen: u64,
    /// Whether this is a dev build.
    #[serde(default)]
    pub dev: bool,
    /// Mesh ID this peer belongs to (if any).
    #[serde(default)]
    pub mesh_id: Option<String>,
    /// Paired peers who told us about this peer via gossip
    /// (instance_ids of vouchers, deduped, sorted). Empty for mDNS/UDP
    /// direct discoveries. UI uses this to surface a "vouched by …, pair?"
    /// affordance instead of a generic Add to Mesh on transitively-known peers.
    #[serde(default)]
    pub vouched_by: Vec<String>,
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
    /// Runtime-only miss counter; not persisted to disk or sent over API.
    #[serde(skip)]
    pub consecutive_misses: u8,
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
    ReadPanes,
    WritePanes,
    SpawnAgents,
    ManageWorkspaces,
    ReadOutput,
    SendInput,
    ManageCrons,
    Admin,
}

// ── Pairing protocol ────────────────────────────────────────────────
// Clean Ed25519-only pairing: exchange public keys, no DH/encrypted tokens.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PairingStatus {
    Pending,
    Accepted,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    pub from_address: String,
    pub from_port: u16,
    /// Ed25519 public key (base64) — the ONLY auth credential exchanged.
    pub public_key: String,
    /// Unix timestamp (seconds). Reject if >60s old.
    pub timestamp: u64,
    #[serde(default = "default_pairing_status")]
    pub status: PairingStatus,
    /// Instance ID of the peer that introduced this request (via /api/mesh/introduce).
    /// None for direct pair requests. Used by the UI to show "Introduced by X".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

fn default_pairing_status() -> PairingStatus {
    PairingStatus::Pending
}

/// Accept response — approver sends their public key back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAccept {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    /// Approver's Ed25519 public key (base64).
    pub public_key: String,
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
}

/// Mesh sync payload — exchanged between paired peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSyncPayload {
    /// Known peers in the mesh (addresses only, no tokens).
    pub peers: Vec<GossipPeerInfo>,
}

// ── Relay ───────────────────────────────────────────────────────────

/// Generic relay — proxy any API call to a mesh peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCommand {
    /// API method: GET, POST, PUT, DELETE, PATCH.
    #[serde(default = "default_method")]
    pub method: String,
    /// API path (e.g. "/api/panes", "/api/agents/spawn").
    pub path: String,
    /// Request body (for POST/PUT/PATCH).
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

// ── Introduction (auto-pair via mediator) ───────────────────────────

/// Peer introduction — sent by a paired mediator to auto-pair two peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerIntroduction {
    /// The peer being introduced.
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    /// Ed25519 public key for signing verification.
    pub public_key: String,
    /// Who introduced them (the mediator's instance_id).
    pub introduced_by: String,
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
    pub discovery_active: bool,
    pub broadcast_active: bool,
    pub broadcast_expires_at: Option<u64>,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    pub paired_count: usize,
    pub discovered_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPeersResponse {
    pub paired: Vec<PairedPeer>,
    pub discovered: Vec<DiscoveredPeer>,
    pub incoming_requests: Vec<PairingRequest>,
}

// ── Revocation ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RevocationList {
    /// Instance IDs whose mesh tokens have been revoked.
    pub revoked: Vec<String>,
    /// Instance IDs blocked from sending pairing requests.
    pub blocked: Vec<String>,
}

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
        assert!(is_lan_private_ip("172.31.255.255"));
        assert!(is_lan_private_ip("192.168.1.1"));
        assert!(is_lan_private_ip("127.0.0.1"));
        assert!(!is_lan_private_ip("172.15.0.1"));
        assert!(!is_lan_private_ip("172.32.0.1"));
        assert!(!is_lan_private_ip("8.8.8.8"));
        assert!(!is_lan_private_ip("100.84.0.1")); // tailscale not LAN
    }

    #[test]
    fn address_tier_orders_tailscale_first_then_lan_then_public() {
        let ts = PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true };
        let lan = PeerAddress { ip: "192.168.1.2".into(), is_tailscale: false };
        let pub_ip = PeerAddress { ip: "8.8.8.8".into(), is_tailscale: false };
        assert!(address_tier(&ts) < address_tier(&lan));
        assert!(address_tier(&lan) < address_tier(&pub_ip));
    }

    #[test]
    fn address_tier_min_picks_tailscale() {
        let addrs = vec![
            PeerAddress { ip: "8.8.8.8".into(), is_tailscale: false },
            PeerAddress { ip: "192.168.1.2".into(), is_tailscale: false },
            PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true },
        ];
        let pick = addrs.iter().min_by_key(|a| address_tier(a)).unwrap();
        assert_eq!(pick.ip, "100.84.1.2");
    }

    #[test]
    fn sanitize_addresses_passes_clean_ips_through() {
        let addrs = vec![
            PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true },
            PeerAddress { ip: "192.168.1.2".into(), is_tailscale: false },
            PeerAddress { ip: "::1".into(), is_tailscale: false },
        ];
        let out = sanitize_addresses(&addrs);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn sanitize_addresses_drops_unparseable_strings() {
        // The actual poison strings observed in the wild from a broken
        // `tailscale ip -4` GUI bridge: error text on stdout instead of an IP.
        let poisoned = vec![
            PeerAddress { ip: "Tailscale daemon is not running.".into(), is_tailscale: false },
            PeerAddress { ip: "failed to connect to local backend".into(), is_tailscale: false },
            PeerAddress { ip: "".into(), is_tailscale: false },
            PeerAddress { ip: "not-an-ip".into(), is_tailscale: false },
        ];
        let out = sanitize_addresses(&poisoned);
        assert!(out.is_empty(), "all entries should be dropped, got: {:?}", out);
    }

    #[test]
    fn sanitize_addresses_preserves_clean_drops_poisoned_in_mixed_list() {
        let mixed = vec![
            PeerAddress { ip: "100.84.1.2".into(), is_tailscale: true },
            PeerAddress { ip: "Tailscale daemon is not running.".into(), is_tailscale: false },
            PeerAddress { ip: "192.168.1.2".into(), is_tailscale: false },
            PeerAddress { ip: "garbage".into(), is_tailscale: false },
        ];
        let out = sanitize_addresses(&mixed);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ip, "100.84.1.2");
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
        // offline→online transition always triggers sync to refresh gossip
        // immediately; previous last_sync timestamp is irrelevant.
        assert!(should_proactive_sync(true, false, 1000, None, 60));
        assert!(should_proactive_sync(true, false, 1000, Some(999), 60));
        assert!(should_proactive_sync(true, false, 1000, Some(0), 60));
    }

    #[test]
    fn proactive_sync_fires_when_last_sync_unknown_for_steady_online_peer() {
        // First iteration after a state where peer was already online but
        // we have no record of having synced — must sync to populate the map.
        assert!(should_proactive_sync(true, true, 1000, None, 60));
    }

    #[test]
    fn proactive_sync_fires_when_interval_elapsed() {
        // Peer steady online and last_sync ≥ interval ago → sync to refresh
        // gossip-discovered peers' last_seen before TTL eviction.
        assert!(should_proactive_sync(true, true, 1060, Some(1000), 60));
        assert!(should_proactive_sync(true, true, 1500, Some(1000), 60));
    }

    #[test]
    fn proactive_sync_skips_when_recently_synced() {
        // Peer steady online and last_sync < interval ago → skip to avoid
        // hammering paired peers. Network traffic stays bounded.
        assert!(!should_proactive_sync(true, true, 1059, Some(1000), 60));
        assert!(!should_proactive_sync(true, true, 1001, Some(1000), 60));
        assert!(!should_proactive_sync(true, true, 1000, Some(1000), 60));
    }

    #[test]
    fn proactive_sync_handles_clock_skew_via_saturating_sub() {
        // If now < last_sync (clock went backwards from system update or
        // similar), saturating_sub yields 0 — interval comparison fails,
        // we skip. Sane behavior: don't blast sync requests after clock skew.
        assert!(!should_proactive_sync(true, true, 500, Some(1000), 60));
    }
}
