use super::channel_failure::INTEGRITY_FAILED;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 1024;

pub(super) fn is_archive_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "https" | "http") && parsed.path().ends_with(".tar.gz")
    })
}

/// A channel published as `<url>` plus its `<url>.sha256`, as release assets anyone can read.
pub(super) fn materialize(
    fetch: impl Fn(&str, usize) -> Result<Vec<u8>, String>,
    url: &str,
    dest: &Path,
) -> Result<(), String> {
    let checksum = fetch(&format!("{url}.sha256"), MAX_CHECKSUM_BYTES)?;
    let expected = parse_checksum(&checksum)
        .ok_or_else(|| format!("{INTEGRITY_FAILED}: malformed published checksum"))?;
    let archive = fetch(url, MAX_ARCHIVE_BYTES)?;
    if format!("{:x}", Sha256::digest(&archive)) != expected {
        return Err(INTEGRITY_FAILED.into());
    }
    unpack(&archive, dest)
}

pub(super) fn parse_checksum(bytes: &[u8]) -> Option<String> {
    let token = std::str::from_utf8(bytes).ok()?.split_whitespace().next()?;
    (token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| token.to_ascii_lowercase())
}

pub(super) fn unpack(archive: &[u8], dest: &Path) -> Result<(), String> {
    let invalid = |error: std::io::Error| format!("invalid channel archive: {error}");
    std::fs::create_dir_all(dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let mut unpacked = 0u64;
    for entry in archive.entries().map_err(invalid)? {
        let mut entry = entry.map_err(invalid)?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        let path = entry.path().map_err(invalid)?.into_owned();
        let relative = contained_path(&path)
            .ok_or_else(|| format!("channel archive entry escapes its root: {}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&relative);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("mkdir {}: {e}", target.display()))?;
        } else if kind.is_file() {
            unpacked += entry.header().size().map_err(invalid)?;
            if unpacked > MAX_UNPACKED_BYTES {
                return Err("channel archive exceeds the unpacked size limit".into());
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let mut file = std::fs::File::create(&target)
                .map_err(|e| format!("create {}: {e}", target.display()))?;
            std::io::copy(&mut entry, &mut file).map_err(invalid)?;
        } else {
            return Err(format!(
                "channel archive entry type is not allowed: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn contained_path(path: &Path) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(relative)
}

#[cfg(test)]
mod tests;
