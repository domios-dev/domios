//! Mesh networking types — peer discovery, pairing, and communication.

use serde::{Deserialize, Serialize};

// ── Identity ────────────────────────────────────────────────────────

/// Persistent identity for this CodeTerm instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshIdentity {
    pub instance_id: String,
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
}

// ── Paired peers ──────────────────────────────────────���─────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedPeer {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<PeerAddress>,
    pub api_port: u16,
    /// Dedicated mesh token for this peer (NOT the main API token).
    pub mesh_token: String,
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
    /// X25519 public key (32 bytes, base64-encoded for JSON).
    pub public_key: String,
    /// AES-GCM nonce (12 bytes, base64-encoded).
    pub nonce: String,
    /// Unix timestamp (seconds). Reject if >60s old.
    pub timestamp: u64,
    #[serde(default = "default_pairing_status")]
    pub status: PairingStatus,
}

fn default_pairing_status() -> PairingStatus {
    PairingStatus::Pending
}

/// Response sent when accepting a pairing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAccept {
    pub request_id: String,
    /// Acceptor's X25519 public key (base64).
    pub public_key: String,
    /// Encrypted mesh token (base64, AES-256-GCM).
    pub encrypted_mesh_token: String,
}

/// Final confirmation with the requester's encrypted mesh token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingConfirm {
    pub request_id: String,
    /// Encrypted mesh token (base64, AES-256-GCM).
    pub encrypted_mesh_token: String,
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
    pub is_crown: bool,
    pub crown_holder: Option<String>,
    pub discovery_active: bool,
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
