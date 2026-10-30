use codeterm_util::watch_plan::PathCase;
use git2::Repository;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) fn paths(repo: &Repository) -> Result<Vec<PathBuf>, String> {
    repo.index()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|entry| index_path(&entry.path))
        .collect()
}

#[cfg(unix)]
fn index_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(Path::new(std::ffi::OsStr::from_bytes(bytes)).to_path_buf())
}

#[cfg(not(unix))]
fn index_path(bytes: &[u8]) -> Result<PathBuf, String> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| error.to_string())
}

pub(crate) struct TrackedPaths {
    keys: BTreeSet<String>,
    case: PathCase,
}

impl TrackedPaths {
    pub(crate) fn read(repo: &Repository, ignore_case: bool) -> Result<Self, String> {
        let case = if ignore_case {
            PathCase::Insensitive
        } else {
            PathCase::Sensitive
        };
        Ok(Self {
            keys: paths(repo)?.iter().map(|path| case.key(path)).collect(),
            case,
        })
    }

    pub(crate) fn contains_or_descendant(&self, path: &Path) -> bool {
        let key = self.case.key(path);
        let prefix = format!("{key}/");
        self.keys.contains(&key)
            || self
                .keys
                .range(prefix.clone()..)
                .next()
                .is_some_and(|candidate| candidate.starts_with(&prefix))
    }
}
