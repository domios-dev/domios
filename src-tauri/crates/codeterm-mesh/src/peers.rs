//! Peer storage — load/save paired peers, identity, and revocation list.
//! Data stored in `~/.codeterm/mesh/`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::types::{MeshIdentity, PairedPeer, RevocationList};

// ── Directory helpers ───────────────────────────────────────────────

/// `~/.codeterm/mesh/` (or `~/.codeterm-dev/mesh/` in dev builds).
pub fn mesh_dir() -> PathBuf {
    let dir = codeterm_paths::data_dir().join("mesh");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn identity_path() -> PathBuf {
    mesh_dir().join("identity.json")
}

fn peers_path() -> PathBuf {
    mesh_dir().join("peers.json")
}

fn revocation_path() -> PathBuf {
    mesh_dir().join("revocation.json")
}

// ── Identity ────────────────────────────────────────────────────────

/// Load or create a stable instance identity (UUID v4).
pub fn load_or_create_identity() -> MeshIdentity {
    let path = identity_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(mut id) = serde_json::from_str::<MeshIdentity>(&data) {
            // Migrate: generate keypair if missing (identity created before Ed25519 support)
            if id.private_key.is_none() || id.public_key.is_none() {
                let (priv_key, pub_key) = super::crown::generate_peer_keypair();
                id.private_key = Some(priv_key);
                id.public_key = Some(pub_key);
                if let Ok(json) = serde_json::to_string_pretty(&id) {
                    let _ = std::fs::write(&path, json);
                }
                log::info!("[mesh] Migrated identity — generated Ed25519 keypair");
            }
            return id;
        }
    }
    let (priv_key, pub_key) = super::crown::generate_peer_keypair();
    let id = MeshIdentity {
        instance_id: uuid::Uuid::new_v4().to_string(),
        private_key: Some(priv_key),
        public_key: Some(pub_key),
    };
    if let Ok(json) = serde_json::to_string_pretty(&id) {
        let _ = std::fs::write(&path, json);
    }
    id
}

/// Persist identity changes (e.g. crown state).
pub fn save_identity(identity: &MeshIdentity) {
    let path = identity_path();
    if let Ok(json) = serde_json::to_string_pretty(identity) {
        let _ = std::fs::write(&path, json);
    }
}

// ── Encryption key ──────────────────────────────────────────────────

// Peer storage.
// PairedPeer serialized directly; no encrypted tokens.

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeersFile {
    peers: HashMap<String, PairedPeerDisk>,
}

/// On-disk peer format (matches PairedPeer minus runtime fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairedPeerDisk {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<super::types::PeerAddress>,
    pub api_port: u16,
    pub public_key: String,
    pub paired_at: String,
    pub last_seen: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub access_level: super::types::AccessLevel,
    pub version: Option<String>,
    #[serde(default)]
    pub web_ui: Option<super::types::WebUiAnnouncement>,
    #[serde(default)]
    pub relay_protocol_version: u16,
}

/// Load all paired peers from disk.
///
/// Runs `sanitize_addresses` on each peer's address list to strip
/// unparseable IPs that pre-v1.1.9 daemons may have published into the
/// record (release/1.1.9 issue #28 / commit f0ff692 background). If
/// anything was filtered, persists the cleaned state immediately so
/// subsequent loads are no-ops. Idempotent.
pub fn load_peers() -> HashMap<String, PairedPeer> {
    let path = peers_path();
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let file: PeersFile = match serde_json::from_str(&data) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Failed to parse peers.json: {}", e);
            return HashMap::new();
        }
    };
    let mut scrubbed = 0usize;
    let peers: HashMap<String, PairedPeer> = file
        .peers
        .into_iter()
        .map(|(id, d)| {
            let before = d.addresses.len();
            let clean = crate::types::sanitize_addresses(&d.addresses);
            scrubbed += before - clean.len();
            (
                id,
                PairedPeer {
                    instance_id: d.instance_id,
                    instance_name: d.instance_name,
                    addresses: clean,
                    api_port: d.api_port,
                    public_key: d.public_key,
                    paired_at: d.paired_at,
                    last_seen: d.last_seen,
                    online: false,
                    is_primary: d.is_primary,
                    access_level: d.access_level,
                    version: d.version,
                    web_ui: d.web_ui,
                    relay_protocol_version: d.relay_protocol_version,
                    consecutive_misses: 0,
                    last_active_ms: None,
                },
            )
        })
        .collect();
    if scrubbed > 0 {
        log::info!(
            "[mesh/migrate] Scrubbed {} unparseable IP entries from peers.json",
            scrubbed
        );
        if let Err(e) = save_peers(&peers) {
            log::warn!(
                "[mesh/migrate] Failed to persist scrubbed peers.json: {}",
                e
            );
        }
    }
    peers
}

/// Save all paired peers to disk.
pub fn save_peers(peers: &HashMap<String, PairedPeer>) -> Result<(), String> {
    let on_disk: HashMap<String, PairedPeerDisk> = peers
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                PairedPeerDisk {
                    instance_id: p.instance_id.clone(),
                    instance_name: p.instance_name.clone(),
                    addresses: p.addresses.clone(),
                    api_port: p.api_port,
                    public_key: p.public_key.clone(),
                    paired_at: p.paired_at.clone(),
                    last_seen: p.last_seen.clone(),
                    is_primary: p.is_primary,
                    access_level: p.access_level.clone(),
                    version: p.version.clone(),
                    web_ui: p.web_ui.clone(),
                    relay_protocol_version: p.relay_protocol_version,
                },
            )
        })
        .collect();
    let file = PeersFile { peers: on_disk };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(peers_path(), json).map_err(|e| e.to_string())
}

// ── Revocation list ─────────────────────────────────────────────────

pub fn load_revocation_list() -> RevocationList {
    let path = revocation_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default()
}

pub fn save_revocation_list(list: &RevocationList) -> Result<(), String> {
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(revocation_path(), json).map_err(|e| e.to_string())
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_stable() {
        let id1 = load_or_create_identity();
        let id2 = load_or_create_identity();
        assert_eq!(id1.instance_id, id2.instance_id);
    }
}
