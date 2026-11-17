use std::path::{Path, PathBuf};

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
        self.validate()?;
        let resolved = codeterm_util::project_roots::canonical_target(self.root())
            .ok_or("Shared storage target cannot be resolved")?;
        let pinned = PinnedSharedReadGrant {
            grant: self,
            resolved,
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
}

impl PinnedSharedReadGrant {
    pub fn resolved(&self) -> &Path {
        &self.resolved
    }

    pub(super) fn allows(&self, target: &Path) -> bool {
        codeterm_util::project_roots::canonical_target(self.grant.root()).as_deref()
            == Some(self.resolved.as_path())
            && self.allows_resolved(target)
    }

    pub(super) fn covers_path(&self, path: &Path, target: &Path) -> bool {
        path.starts_with(self.grant.root()) || target.starts_with(&self.resolved)
    }

    fn allows_resolved(&self, target: &Path) -> bool {
        match &self.grant {
            SharedReadGrant::Directory { .. } => target.starts_with(&self.resolved),
            SharedReadGrant::File { .. } => target == self.resolved,
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
