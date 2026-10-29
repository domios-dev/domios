use std::io::{Read, Write};
use std::path::Path;

use super::{LatestRelease, ResolvedManifest, UPDATE_REPOSITORIES};

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredManifest {
    repository: String,
    manifest: crate::manifest::Manifest,
}

pub(super) fn publish(
    root: &Path,
    key: &str,
    fresh: Option<ResolvedManifest>,
    errors: &[String],
) -> Result<ResolvedManifest, String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("{key}.lock")))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("Update manifest cache unavailable: {e}"))?;
    let path = root.join(key);
    let cached = read(&path);
    let resolved = super::reuse_manifest(fresh, cached, errors)?;
    let stored = StoredManifest {
        repository: resolved.release.repository.into(),
        manifest: resolved.manifest.clone(),
    };
    let bytes = serde_json::to_vec(&stored).map_err(|e| e.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary.persist(&path).map_err(|e| e.to_string())?;
    Ok(resolved)
}

fn read(path: &Path) -> Option<ResolvedManifest> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let stored: StoredManifest = serde_json::from_slice(&bytes).ok()?;
    semver::Version::parse(&stored.manifest.version).ok()?;
    let repository = UPDATE_REPOSITORIES
        .into_iter()
        .find(|repository| *repository == stored.repository)?;
    Some(ResolvedManifest {
        release: LatestRelease {
            repository,
            tag: format!("v{}", stored.manifest.version),
        },
        manifest: stored.manifest,
        refresh_error: None,
    })
}
