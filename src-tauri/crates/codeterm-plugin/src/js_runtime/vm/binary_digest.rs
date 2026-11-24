use std::{
    fs::File,
    path::{Path, PathBuf},
    time::SystemTime,
};

use codeterm_util::fs_links::{identity, FileIdentity};
use parking_lot::Mutex;

#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    identity: FileIdentity,
    bytes: u64,
    modified: SystemTime,
}

impl FileStamp {
    fn read(file: &File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        Some(Self {
            identity: identity(file).ok()?,
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

struct CachedDigest {
    path: PathBuf,
    stamp: FileStamp,
    digest: String,
}

#[derive(Default)]
pub(super) struct BinaryDigestCache(Mutex<Option<CachedDigest>>);

impl BinaryDigestCache {
    pub(super) fn digest(&self, path: &Path) -> Option<String> {
        self.read_with(path, crate::model_catalog::binary_fingerprint_file)
    }

    fn read_with(
        &self,
        path: &Path,
        hash: impl FnOnce(&mut File) -> Option<String>,
    ) -> Option<String> {
        let Ok(path) = path.canonicalize() else {
            return hash(&mut File::open(path).ok()?);
        };
        // The VM holds at most one CLI and this lock only serializes its digest readers.
        let mut cached = self.0.lock();
        let mut file = File::open(&path).ok()?;
        let Some(stamp) = FileStamp::read(&file) else {
            return hash(&mut file);
        };
        if let Some(entry) = cached
            .as_ref()
            .filter(|entry| entry.path == path && entry.stamp == stamp)
        {
            return Some(entry.digest.clone());
        }
        let digest = hash(&mut file)?;
        if FileStamp::read(&file).as_ref() != Some(&stamp) {
            return None;
        }
        *cached = Some(CachedDigest {
            path,
            stamp,
            digest: digest.clone(),
        });
        Some(digest)
    }
}

#[cfg(test)]
mod tests;
