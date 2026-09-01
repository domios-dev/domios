use super::events::RepoWatchError;
use git2::Repository;
use std::path::{Path, PathBuf};

/// Identity is the canonical WORKTREE root: a linked worktree has its own
/// HEAD, index and dirty state, so it is its own bubble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    /// Registry key — case-folded when the repository itself reports
    /// `core.ignorecase`, so path aliases collapse to one entry.
    pub key: String,
    /// Display identity handed to subscribers and carried on every event.
    pub id: String,
    pub workdir: PathBuf,
    pub gitdir: PathBuf,
    pub common_dir: PathBuf,
    pub common_key: String,
    pub ignore_case: bool,
}

impl RepoIdentity {
    /// Resolve a path to its repository identity. `discover` handles a `.git`
    /// FILE and gitdir indirection, so linked worktrees need no hand-parsing.
    pub fn resolve(path: &Path) -> Result<Self, RepoWatchError> {
        let repo = Repository::discover(path)
            .map_err(|e| RepoWatchError::not_a_repository(path, e.message().to_string()))?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| RepoWatchError::not_a_repository(path, "bare repository has no worktree"))?;
        let workdir = normalize(workdir)
            .map_err(|e| RepoWatchError::not_a_repository(path, e.to_string()))?;
        let gitdir =
            normalize(repo.path()).map_err(|e| RepoWatchError::not_a_repository(path, e.to_string()))?;
        let common_dir = common_dir(&gitdir);
        let ignore_case = repo
            .config()
            .and_then(|c| c.get_bool("core.ignorecase"))
            .unwrap_or(cfg!(any(windows, target_os = "macos")));
        let id = workdir.to_string_lossy().to_string();
        Ok(Self {
            key: fold(&id, ignore_case),
            id,
            workdir,
            gitdir,
            common_key: fold(&common_dir.to_string_lossy(), ignore_case),
            common_dir,
            ignore_case,
        })
    }

    /// Re-open the repository for a batch. A failure here is the repository or
    /// worktree disappearing while watched, never a reason to keep emitting.
    pub fn open(&self) -> Result<Repository, RepoWatchError> {
        Repository::open_ext(
            &self.workdir,
            git2::RepositoryOpenFlags::NO_SEARCH,
            std::iter::empty::<&std::ffi::OsStr>(),
        )
        .map_err(|e| RepoWatchError::repository_gone(&self.id, e.message().to_string()))
    }
}

fn fold(value: &str, ignore_case: bool) -> String {
    if ignore_case {
        value.to_lowercase()
    } else {
        value.to_string()
    }
}

/// libgit2 resolves a linked worktree's shared store through this pointer file.
/// git2 0.19 exposes no `commondir()` — libgit2-sys 0.17 omits the binding.
fn common_dir(gitdir: &Path) -> PathBuf {
    let Ok(raw) = std::fs::read_to_string(gitdir.join("commondir")) else {
        return gitdir.to_path_buf();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return gitdir.to_path_buf();
    }
    let joined = gitdir.join(trimmed);
    normalize(&joined).unwrap_or(joined)
}

/// Canonical form: `..` segments resolved, symlinks followed, and on Windows
/// the true on-disk casing, so aliases of one root collapse to one key.
pub fn normalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(strip_verbatim(&canonical))
}

#[cfg(windows)]
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(not(windows))]
fn strip_verbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}
