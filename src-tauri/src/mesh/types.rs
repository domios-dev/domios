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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAddress {
    pub ip: String,
    pub is_tailscale: bool,
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
}

// ── Paired peers ──────────────────────────────────────���─────────────

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
    /// Trust level from crown's signed declarations.
    #[serde(default, skip_deserializing)]
    pub trust_level: String,
    pub version: Option<String>,
}

// ── Access levels (future-proof) ────────────────────────────────────

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

/// Mesh sync payload — exchanged after pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSyncPayload {
    /// Known peers in the mesh (addresses only, no tokens).
    pub peers: Vec<GossipPeerInfo>,
    /// Crown public info (so new peer knows who's crowned).
    pub crown_public: Option<super::crown::CrownPublicInfo>,
    /// Trust declarations (signed by crown, verifiable).
    pub trust_declarations: Vec<super::crown::TrustDeclaration>,
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

/// Peer introduction — sent by a trusted mediator to auto-pair two peers.
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
    pub is_crown: bool,
    pub crown_holder: Option<String>,
    pub discovery_active: bool,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    pub api_port: u16,
    pub can_manage: bool,
    pub paired_count: usize,
    pub discovered_count: usize,
    /// Our own trust level in the mesh: "trusted", "observer", "untrusted", or "crown"
    pub trust_level: String,
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
