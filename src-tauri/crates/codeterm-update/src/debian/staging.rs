use super::{staged_command, verify, StagedDebianPackage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, time::Duration};

const PREFIX: &str = "domios-verified-update-";

pub struct StageLease(File);

impl StageLease {
    pub fn acquire(directory: &Path) -> Result<Self, String> {
        if !owned(directory) {
            return Err("staged directory is not owned by this user".into());
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("in-use"))
            .map_err(|e| e.to_string())?;
        file.try_lock().map_err(|e| e.to_string())?;
        Ok(Self(file))
    }
}

impl Drop for StageLease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: String,
    sha256: String,
    signature: String,
}

pub(super) fn digest(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn owned(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    let correct_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|name| name.starts_with(PREFIX));
    #[cfg(unix)]
    let correct_owner = {
        use std::os::unix::fs::MetadataExt;
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0
    };
    #[cfg(not(unix))]
    let correct_owner = true;
    correct_name && meta.is_dir() && correct_owner
}

pub(super) fn store(
    bytes: &[u8],
    version: &str,
    signature: &str,
    root: &Path,
) -> Result<StagedDebianPackage, String> {
    let mut directory = tempfile::Builder::new();
    directory.prefix(PREFIX);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let dir = directory.tempdir_in(root).map_err(|e| e.to_string())?;
    let lease = StageLease::acquire(dir.path())?;
    let path = dir.path().join("domios-update.deb");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let sha256 = digest(&path)?;
    let metadata = Metadata {
        version: version.into(),
        sha256: sha256.clone(),
        signature: signature.into(),
    };
    std::fs::write(
        dir.path().join("verified.json"),
        serde_json::to_vec(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let command = staged_command(&path)?;
    let _ = dir.keep();
    Ok(StagedDebianPackage {
        path,
        command,
        version: version.into(),
        sha256,
        lease,
    })
}

pub(super) fn reuse(
    root: &Path,
    version: &str,
    signature: &str,
    public_key: &str,
) -> Result<Option<StagedDebianPackage>, String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(None);
    };
    for entry in entries.flatten() {
        let directory = entry.path();
        if !owned(&directory) {
            continue;
        }
        let Some(metadata) = std::fs::read(directory.join("verified.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Metadata>(&bytes).ok())
        else {
            continue;
        };
        if metadata.version != version || metadata.signature != signature {
            continue;
        }
        let lease = StageLease::acquire(&directory).map_err(|_| "already_in_flight".to_string())?;
        let path = directory.join("domios-update.deb");
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if verify(&bytes, signature, public_key).is_err()
            || digest(&path).ok().as_ref() != Some(&metadata.sha256)
        {
            continue;
        }
        return Ok(Some(StagedDebianPackage {
            command: staged_command(&path)?,
            path,
            version: metadata.version,
            sha256: metadata.sha256,
            lease,
        }));
    }
    Ok(None)
}

fn cleanup_selected(owned: bool, in_use: bool, age: Duration) -> bool {
    owned && !in_use && age >= Duration::from_secs(24 * 60 * 60)
}

pub fn cleanup(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !owned(&path) {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .unwrap_or_default();
        let lease = StageLease::acquire(&path);
        if cleanup_selected(true, lease.is_err(), age) {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                log::debug!("[updater] stale package cleanup failed: {error}");
            }
        }
    }
}

pub fn failure(data: &Path) -> Option<String> {
    let path = data.join("debian-update-failure.txt");
    let message = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(message)
}

#[cfg(any(test, target_os = "linux"))]
pub(super) fn instance_unused(paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("instance ownership paths are unavailable".into());
    }
    let mut locks = Vec::new();
    for path in paths {
        let mut options = File::options();
        options.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("instance ownership path is not a file".into());
        }
        file.try_lock()
            .map_err(|_| "a server still owns the data instance; rollback withheld".to_string())?;
        locks.push(file);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
