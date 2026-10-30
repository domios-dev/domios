//! Stable mesh identity and targeted removal of obsolete pair storage.
//! Data stored in `~/.domios/mesh/`.

use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};
use serde::de::DeserializeOwned;

use super::types::MeshIdentity;

// ── Directory helpers ───────────────────────────────────────────────

/// `~/.domios/mesh/` (or `~/.domios-dev/mesh/` in dev builds).
pub fn mesh_dir() -> PathBuf {
    let dir = codeterm_paths::data_dir().join("mesh");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn identity_path() -> PathBuf {
    mesh_dir().join("identity.json")
}

// ── Identity ────────────────────────────────────────────────────────

fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

fn validate_json<T: DeserializeOwned>(text: &str) -> Result<(), String> {
    parse_json::<T>(text).map(|_| ())
}

fn identity_file(path: &Path) -> StateFile {
    StateFile::new(path)
        .with_private_permissions()
        .with_validator(validate_json::<MeshIdentity>)
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

#[derive(Default)]
pub struct LegacyPairRemoval {
    pub removed: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

pub fn remove_legacy_pair_store() -> LegacyPairRemoval {
    remove_legacy_pair_store_at(&mesh_dir())
}

pub(crate) fn remove_legacy_pair_store_at(directory: &Path) -> LegacyPairRemoval {
    let mut result = LegacyPairRemoval::default();
    for name in ["peers.json", "account_mesh_ledger.json"] {
        let path = directory.join(name);
        let file = StateFile::new(&path);
        let artifacts = std::iter::once(path)
            .chain(file.backups())
            .chain(file.corrupt_copies());
        for artifact in artifacts {
            match std::fs::remove_file(&artifact) {
                Ok(()) => result.removed.push(artifact),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => result.failed.push((artifact, error.to_string())),
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
