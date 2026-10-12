use super::events::RepoWatchError;
use super::identity::RepoIdentity;
use git2::Repository;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Which watch produced a batch. The two halves have disjoint relevant sets,
/// so a primary repository (gitdir == commondir) never emits twice for one change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchHalf {
    Worktree,
    SharedRefs,
}

/// Does this batch contain anything `git status --porcelain` or the branch
/// label would show? Ignored churn and `.git` internals answer `false`.
pub fn batch_is_relevant(
    identity: &RepoIdentity,
    half: WatchHalf,
    paths: &[PathBuf],
) -> Result<bool, RepoWatchError> {
    match half {
        WatchHalf::SharedRefs => Ok(paths.iter().any(|p| shared_ref_relevant(identity, p))),
        WatchHalf::Worktree => worktree_batch_is_relevant(identity, paths),
    }
}

fn shared_ref_relevant(identity: &RepoIdentity, path: &Path) -> bool {
    if is_lock(path) {
        return false;
    }
    let Some(rest) = relative_to(&identity.common_dir, path, identity.ignore_case) else {
        return false;
    };
    // `refs/` recursively, plus `packed-refs`: a ref update may land in the
    // packed file, and watching `refs/` alone misses it.
    rest == Path::new("packed-refs") || rest.starts_with("refs")
}

fn worktree_batch_is_relevant(
    identity: &RepoIdentity,
    paths: &[PathBuf],
) -> Result<bool, RepoWatchError> {
    let mut workdir_candidates: Vec<&PathBuf> = Vec::new();
    for path in paths {
        if git_meta_relevant(identity, path) {
            return Ok(true);
        }
        if is_git_internal(identity, path) {
            continue;
        }
        if relative_to(&identity.workdir, path, identity.ignore_case).is_some() {
            workdir_candidates.push(path);
        }
    }
    if workdir_candidates.is_empty() {
        return Ok(false);
    }
    // Opening here is also how a deleted repository or worktree is detected.
    let repo = identity.open()?;
    let mut memo: HashMap<PathBuf, bool> = HashMap::new();
    for path in workdir_candidates {
        if !is_ignored(&repo, identity, path, &mut memo) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The only `.git` paths that move a bubble: this worktree's own HEAD and index.
fn git_meta_relevant(identity: &RepoIdentity, path: &Path) -> bool {
    if is_lock(path) {
        return false;
    }
    let Some(rest) = relative_to(&identity.gitdir, path, identity.ignore_case) else {
        return false;
    };
    rest == Path::new("HEAD") || rest == Path::new("index")
}

fn is_git_internal(identity: &RepoIdentity, path: &Path) -> bool {
    relative_to(&identity.gitdir, path, identity.ignore_case).is_some()
        || relative_to(&identity.common_dir, path, identity.ignore_case).is_some()
        || relative_to(&identity.workdir.join(".git"), path, identity.ignore_case).is_some()
}

fn is_ignored(
    repo: &Repository,
    identity: &RepoIdentity,
    path: &Path,
    memo: &mut HashMap<PathBuf, bool>,
) -> bool {
    let Some(rel) = relative_to(&identity.workdir, path, identity.ignore_case) else {
        return true;
    };
    if rel.as_os_str().is_empty() {
        // The worktree root itself — a deletion event; `identity.open()` above
        // already proved the repository is still there, so nothing to report.
        return true;
    }
    // Collapse a burst first: one directory probe answers every file under an
    // ignored tree, so a 500-file `target/` churn costs one ignore match.
    if let Some(parent) = rel.parent() {
        if !parent.as_os_str().is_empty() {
            let cached = memo.get(parent).copied();
            let ignored = match cached {
                Some(value) => value,
                None => {
                    let probe = format!("{}/", parent.to_string_lossy().replace('\\', "/"));
                    let value = repo
                        .status_should_ignore(Path::new(&probe))
                        .unwrap_or(false);
                    memo.insert(parent.to_path_buf(), value);
                    value
                }
            };
            if ignored {
                return true;
            }
        }
    }
    repo.status_should_ignore(&rel).unwrap_or(false)
}

fn is_lock(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "lock")
}

fn relative_to(base: &Path, path: &Path, ignore_case: bool) -> Option<PathBuf> {
    if let Ok(rest) = path.strip_prefix(base) {
        return Some(rest.to_path_buf());
    }
    if !ignore_case {
        return None;
    }
    let base_text = base.to_string_lossy().to_lowercase();
    let path_text = path.to_string_lossy().to_lowercase();
    let rest = path_text.strip_prefix(&base_text)?;
    // A prefix that does not end on a component boundary is a different path
    // ("/repo" is not a parent of "/repository").
    if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
        return None;
    }
    Some(PathBuf::from(rest.trim_start_matches(['/', '\\'])))
}
