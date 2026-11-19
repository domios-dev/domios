use codeterm_util::fs_links::{file_identity, FileIdentity};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;

use super::storage::{SharedStoragePath, StoragePathKind};

#[derive(Debug, Clone, Serialize)]
pub enum SharedReadGrant {
    Directory { path: PathBuf },
    File { path: PathBuf },
    VersionedSqlite { directory: PathBuf, stem: String },
}

impl SharedReadGrant {
    pub fn from_layout(layout: &SharedStoragePath, base: &Path) -> Result<Self, String> {
        if !base.is_absolute()
            || layout.path.is_empty()
            || layout.path.starts_with(['/', '\\'])
            || layout
                .path
                .split(['/', '\\'])
                .any(|part| matches!(part, "." | "..") || part.contains([':', '\0']))
        {
            return Err("shared storage path must remain inside its declared root".into());
        }
        let path = layout
            .path
            .split(['/', '\\'])
            .fold(base.to_owned(), |root, part| root.join(part));
        let grant = match layout.kind {
            StoragePathKind::Directory => Self::Directory { path },
            StoragePathKind::File => Self::File { path },
            StoragePathKind::VersionedSqlite => Self::VersionedSqlite {
                directory: path
                    .parent()
                    .ok_or("shared SQLite directory is required")?
                    .into(),
                stem: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("shared SQLite stem is required")?
                    .into(),
            },
        };
        grant.validate()?;
        Ok(grant)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        let path = match self {
            Self::Directory { path } | Self::File { path } => path,
            Self::VersionedSqlite { directory, stem } => {
                if stem.is_empty() || stem.contains(['/', '\\', ':', '\0', '*', '?']) {
                    return Err("invalid shared SQLite filename stem".into());
                }
                directory
            }
        };
        if !path.is_absolute() {
            return Err("shared storage authority requires an absolute path".into());
        }
        Ok(())
    }

    pub fn pin(self, exclusions: &[PathBuf]) -> Result<PinnedSharedReadGrant, String> {
        self.pin_with_identities(exclusions, &[])
    }

    pub(super) fn pin_with_identities(
        self,
        exclusions: &[PathBuf],
        retained: &[FileIdentity],
    ) -> Result<PinnedSharedReadGrant, String> {
        self.validate()?;
        let resolved = codeterm_util::project_roots::canonical_target(self.root())
            .ok_or("Shared storage target cannot be resolved")?;
        let held = if matches!(self, Self::File { .. }) && resolved.is_file() {
            Some(Arc::new(
                File::open(&resolved).map_err(|error| error.to_string())?,
            ))
        } else {
            None
        };
        let identity = held
            .as_ref()
            .map(|file| codeterm_util::fs_links::identity(file))
            .transpose()
            .map_err(|error| error.to_string())?;
        let pinned = PinnedSharedReadGrant {
            grant: self,
            resolved,
            identity,
            _held: held,
        };
        for exclusion in exclusions {
            let exclusion = codeterm_util::project_roots::canonical_target(exclusion)
                .ok_or("Isolated storage target cannot be resolved")?;
            if pinned.resolved.starts_with(&exclusion)
                || pinned.allows_resolved(&exclusion)
                || (matches!(pinned.grant, Self::Directory { .. })
                    && exclusion.starts_with(&pinned.resolved))
            {
                return Err("Shared history overlaps isolated account storage".into());
            }
        }
        let mut identities = exclusions
            .iter()
            .map(|path| file_identity(path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        identities.extend_from_slice(retained);
        reject_isolated_aliases(&pinned, exclusions, &identities)?;
        Ok(pinned)
    }

    fn root(&self) -> &Path {
        match self {
            Self::Directory { path } | Self::File { path } => path,
            Self::VersionedSqlite { directory, .. } => directory,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct PinnedSharedReadGrant {
    grant: SharedReadGrant,
    resolved: PathBuf,
    identity: Option<FileIdentity>,
    #[serde(skip)]
    _held: Option<Arc<File>>,
}

impl PinnedSharedReadGrant {
    pub fn resolved(&self) -> &Path {
        &self.resolved
    }

    pub(super) fn directory_target(&self) -> Option<&Path> {
        matches!(self.grant, SharedReadGrant::Directory { .. }).then_some(self.resolved.as_path())
    }

    pub(super) fn allows(&self, target: &Path) -> bool {
        codeterm_util::project_roots::canonical_target(self.grant.root()).as_deref()
            == Some(self.resolved.as_path())
            && self.identity.is_none_or(|expected| {
                file_identity(self.grant.root()).ok().flatten() == Some(expected)
            })
            && self.allows_resolved(target)
    }

    pub(super) fn covers_path(&self, path: &Path, target: &Path) -> bool {
        path.starts_with(self.grant.root()) || target.starts_with(&self.resolved)
    }

    fn allows_resolved(&self, target: &Path) -> bool {
        match &self.grant {
            SharedReadGrant::Directory { .. } => target.starts_with(&self.resolved),
            SharedReadGrant::File { .. } => {
                target == self.resolved
                    || self.identity.is_some_and(|expected| {
                        file_identity(target).ok().flatten() == Some(expected)
                    })
            }
            SharedReadGrant::VersionedSqlite { stem, .. } => {
                target == self.resolved
                    || (target.parent() == Some(self.resolved.as_path())
                        && target
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| versioned_sqlite_name(stem, name)))
            }
        }
    }
}

fn versioned_sqlite_name(stem: &str, name: &str) -> bool {
    let name = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    name.strip_prefix(stem)
        .and_then(|name| name.strip_suffix(".sqlite"))
        .is_some_and(|version| {
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn reject_isolated_aliases(
    grant: &PinnedSharedReadGrant,
    exclusions: &[PathBuf],
    identities: &[FileIdentity],
) -> Result<(), String> {
    let mut paths = vec![grant.resolved.clone()];
    let mut visited = std::collections::HashSet::new();
    let mut count = 0;
    while let Some(path) = paths.pop() {
        count += 1;
        if count > 65_536 {
            return Err("Shared history inspection exceeds its entry bound".into());
        }
        let target = codeterm_util::project_roots::canonical_target(&path)
            .ok_or("Shared history target cannot be resolved")?;
        if exclusions
            .iter()
            .any(|excluded| target.starts_with(excluded))
        {
            return Err("Shared history overlaps isolated account storage".into());
        }
        if !visited.insert(target) {
            continue;
        }
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|error| error.to_string())? {
                let path = entry.map_err(|error| error.to_string())?.path();
                if let SharedReadGrant::VersionedSqlite { stem, .. } = &grant.grant {
                    if !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| versioned_sqlite_name(stem, name))
                    {
                        continue;
                    }
                }
                paths.push(path);
            }
        } else if file_identity(&path)
            .map_err(|error| error.to_string())?
            .is_some_and(|identity| identities.contains(&identity))
        {
            return Err(
                "Shared history aliases an isolated credential or configuration file".into(),
            );
        }
    }
    Ok(())
}
