//! Peer storage — load/save paired peers, identity, and revocation list.
//! Data stored in `~/.codeterm/mesh/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};
use serde::de::DeserializeOwned;
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

fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

fn validate_json<T: DeserializeOwned>(text: &str) -> Result<(), String> {
    parse_json::<T>(text).map(|_| ())
}

fn identity_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate_json::<MeshIdentity>)
}

fn fresh_identity() -> MeshIdentity {
    let (priv_key, pub_key) = super::crown::generate_peer_keypair();
    MeshIdentity {
        instance_id: uuid::Uuid::new_v4().to_string(),
        private_key: Some(priv_key),
        public_key: Some(pub_key),
    }
}

fn write_identity(file: &StateFile, identity: &MeshIdentity) {
    let result = serde_json::to_string_pretty(identity)
        .map_err(|e| e.to_string())
        .and_then(|json| file.save(&json));
    if let Err(e) = result {
        log::warn!("[mesh] identity not saved: {}", e);
    }
}

/// Load or create a stable instance identity (UUID v4).
pub fn load_or_create_identity() -> MeshIdentity {
    load_or_create_identity_at(&identity_path())
}

pub(crate) fn load_or_create_identity_at(path: &Path) -> MeshIdentity {
    let file = identity_file(path);
    match file.load(parse_json::<MeshIdentity>) {
        Loaded::Parsed(mut id) | Loaded::Recovered { value: mut id, .. } => {
            // Migrate: generate keypair if missing (identity created before Ed25519 support)
            if id.private_key.is_none() || id.public_key.is_none() {
                let (priv_key, pub_key) = super::crown::generate_peer_keypair();
                id.private_key = Some(priv_key);
                id.public_key = Some(pub_key);
                write_identity(&file, &id);
                log::info!("[mesh] Migrated identity — generated Ed25519 keypair");
            }
            id
        }
        Loaded::Missing => {
            let id = fresh_identity();
            write_identity(&file, &id);
            id
        }
        // Paired peers pin the stored key, so a held identity is never replaced on disk.
        Loaded::Unrecoverable { .. } => fresh_identity(),
    }
}

/// Persist identity changes (e.g. crown state).
pub fn save_identity(identity: &MeshIdentity) {
    write_identity(&identity_file(&identity_path()), identity);
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

/// Rehydrate a `peers.json` payload without filesystem access.
/// Returns the peers and the number of invalid addresses removed.
pub(crate) fn rehydrate_peers_payload(
    data: &str,
) -> Result<(HashMap<String, PairedPeer>, usize), serde_json::Error> {
    let file: PeersFile = serde_json::from_str(data)?;
    let mut scrubbed = 0usize;
    let peers = file
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
    Ok((peers, scrubbed))
}

/// Load all paired peers from disk.
///
/// Runs `sanitize_addresses` on each peer's address list to strip
/// unparseable IPs that pre-v1.1.9 daemons may have published into the
/// record. If anything was filtered, persists the cleaned state immediately so
/// subsequent loads are no-ops. Idempotent.
pub fn load_peers() -> HashMap<String, PairedPeer> {
    load_peers_at(&peers_path())
}

fn peers_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate_peers)
}

fn validate_peers(text: &str) -> Result<(), String> {
    rehydrate_peers_payload(text)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) fn load_peers_at(path: &Path) -> HashMap<String, PairedPeer> {
    let (peers, scrubbed) = match peers_file(path)
        .load(|text| rehydrate_peers_payload(text).map_err(|e| e.to_string()))
    {
        Loaded::Parsed(result) | Loaded::Recovered { value: result, .. } => result,
        Loaded::Missing | Loaded::Unrecoverable { .. } => return HashMap::new(),
    };
    if scrubbed > 0 {
        log::info!(
            "[mesh/migrate] Scrubbed {} unparseable IP entries from peers.json",
            scrubbed
        );
        if let Err(e) = save_peers_at(path, &peers) {
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
    save_peers_at(&peers_path(), peers)
}

pub(crate) fn save_peers_at(
    path: &Path,
    peers: &HashMap<String, PairedPeer>,
) -> Result<(), String> {
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
    peers_file(path).save(&json)
}

// ── Revocation list ─────────────────────────────────────────────────

fn revocation_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate_json::<RevocationList>)
}

/// A held revocation list loads empty but is never saved over, so revoked
/// peers stay revoked on disk.
pub fn load_revocation_list() -> RevocationList {
    load_revocation_list_at(&revocation_path())
}

pub(crate) fn load_revocation_list_at(path: &Path) -> RevocationList {
    match revocation_file(path).load(parse_json::<RevocationList>) {
        Loaded::Parsed(list) | Loaded::Recovered { value: list, .. } => list,
        Loaded::Missing | Loaded::Unrecoverable { .. } => RevocationList::default(),
    }
}

pub fn save_revocation_list(list: &RevocationList) -> Result<(), String> {
    save_revocation_list_at(&revocation_path(), list)
}

pub(crate) fn save_revocation_list_at(path: &Path, list: &RevocationList) -> Result<(), String> {
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    revocation_file(path).save(&json)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
