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

    pub(super) fn allows(&self, target: &Path) -> bool {
        let canonical = codeterm_util::project_roots::canonical_target;
        match self {
            Self::Directory { path } => {
                canonical(path).is_some_and(|root| target.starts_with(root))
            }
            Self::File { path } => canonical(path).as_deref() == Some(target),
            Self::VersionedSqlite { directory, stem } => canonical(directory).is_some_and(|root| {
                target == root
                    || (target.parent() == Some(root.as_path())
                        && target
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| versioned_sqlite_name(stem, name)))
            }),
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
