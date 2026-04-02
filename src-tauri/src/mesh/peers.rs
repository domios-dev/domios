//! Peer storage — load/save paired peers and revocation list.
//!
//! Data is stored in `~/.codeterm/mesh/peers.json`.
//! Mesh tokens are encrypted at rest using AES-256-GCM with a
//! machine-local key from `~/.codeterm/mesh/.mesh_key`.

use std::collections::HashMap;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::types::{MeshIdentity, PairedPeer, RevocationList};

const B64: base64::engine::general_purpose::GeneralPurpose = base64::engine::general_purpose::STANDARD;

// ── Directory helpers ───────────────────────────────────────────────

/// `~/.codeterm/mesh/` (or `~/.codeterm-dev/mesh/` in dev builds).
pub fn mesh_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("mesh");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn identity_path() -> PathBuf {
    mesh_dir().join("identity.json")
}

fn peers_path() -> PathBuf {
    mesh_dir().join("peers.json")
}

fn mesh_key_path() -> PathBuf {
    mesh_dir().join(".mesh_key")
}

fn revocation_path() -> PathBuf {
    mesh_dir().join("revocation.json")
}

// ── Identity ────────────────────────────────────────────────────────

/// Load or create a stable instance identity (UUID v4).
pub fn load_or_create_identity() -> MeshIdentity {
    let path = identity_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(id) = serde_json::from_str::<MeshIdentity>(&data) {
            return id;
        }
    }
    let id = MeshIdentity {
        instance_id: uuid::Uuid::new_v4().to_string(),
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

/// Load or generate the machine-local AES-256 key (32 bytes).
fn load_or_create_mesh_key() -> [u8; 32] {
    let path = mesh_key_path();
    if let Ok(data) = std::fs::read(&path) {
        if data.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&data);
            return key;
        }
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let _ = std::fs::write(&path, &key);
    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    key
}

/// Encrypt a string with the mesh key. Returns `nonce:ciphertext` in base64.
pub fn encrypt_token(plaintext: &str) -> Result<String, String> {
    let key = load_or_create_mesh_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}:{}",
        B64.encode(nonce_bytes),
        B64.encode(ciphertext)
    ))
}

/// Decrypt a `nonce:ciphertext` string with the mesh key.
pub fn decrypt_token(encrypted: &str) -> Result<String, String> {
    let parts: Vec<&str> = encrypted.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err("Invalid encrypted token format".to_string());
    }
    let nonce_bytes = B64.decode(parts[0]).map_err(|e| e.to_string())?;
    let ciphertext = B64.decode(parts[1]).map_err(|e| e.to_string())?;
    if nonce_bytes.len() != 12 {
        return Err("Invalid nonce length".to_string());
    }
    let key = load_or_create_mesh_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| e.to_string())?;
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

// ── Peer storage ────────────────────────────────────────────────────

/// On-disk format: mesh_token is stored encrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeersFile {
    peers: HashMap<String, PairedPeerOnDisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairedPeerOnDisk {
    #[serde(flatten)]
    pub peer: PairedPeerFlat,
    /// Encrypted mesh_token (`nonce:ciphertext` in base64).
    pub mesh_token_encrypted: String,
}

/// PairedPeer without the `mesh_token` field (for flat serialization).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairedPeerFlat {
    pub instance_id: String,
    pub instance_name: String,
    pub addresses: Vec<super::types::PeerAddress>,
    pub api_port: u16,
    pub paired_at: String,
    pub last_seen: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub access_level: super::types::AccessLevel,
    pub version: Option<String>,
}

/// Load all paired peers from disk, decrypting mesh tokens.
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
    let mut result = HashMap::new();
    for (id, on_disk) in file.peers {
        let mesh_token = match decrypt_token(&on_disk.mesh_token_encrypted) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("Failed to decrypt mesh token for {}: {}", id, e);
                continue;
            }
        };
        result.insert(
            id,
            PairedPeer {
                instance_id: on_disk.peer.instance_id,
                instance_name: on_disk.peer.instance_name,
                addresses: on_disk.peer.addresses,
                api_port: on_disk.peer.api_port,
                mesh_token,
                paired_at: on_disk.peer.paired_at,
                last_seen: on_disk.peer.last_seen,
                online: false, // runtime state, not persisted
                is_primary: on_disk.peer.is_primary,
                access_level: on_disk.peer.access_level,
                version: on_disk.peer.version,
            },
        );
    }
    result
}

/// Save all paired peers to disk, encrypting mesh tokens.
pub fn save_peers(peers: &HashMap<String, PairedPeer>) -> Result<(), String> {
    let mut on_disk = HashMap::new();
    for (id, peer) in peers {
        let encrypted = encrypt_token(&peer.mesh_token)?;
        on_disk.insert(
            id.clone(),
            PairedPeerOnDisk {
                peer: PairedPeerFlat {
                    instance_id: peer.instance_id.clone(),
                    instance_name: peer.instance_name.clone(),
                    addresses: peer.addresses.clone(),
                    api_port: peer.api_port,
                    paired_at: peer.paired_at.clone(),
                    last_seen: peer.last_seen.clone(),
                    is_primary: peer.is_primary,
                    access_level: peer.access_level.clone(),
                    version: peer.version.clone(),
                },
                mesh_token_encrypted: encrypted,
            },
        );
    }
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
    fn test_encrypt_decrypt_roundtrip() {
        let token = "test-mesh-token-12345";
        let encrypted = encrypt_token(token).unwrap();
        assert_ne!(encrypted, token);
        assert!(encrypted.contains(':'));
        let decrypted = decrypt_token(&encrypted).unwrap();
        assert_eq!(decrypted, token);
    }

    #[test]
    fn test_decrypt_invalid_format() {
        assert!(decrypt_token("no-colon-here").is_err());
        assert!(decrypt_token("bad:data").is_err());
    }

    #[test]
    fn test_identity_stable() {
        let id1 = load_or_create_identity();
        let id2 = load_or_create_identity();
        assert_eq!(id1.instance_id, id2.instance_id);
    }
}
