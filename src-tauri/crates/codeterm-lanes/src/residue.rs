use std::path::{Path, PathBuf};

use crate::lane_removal::{find_lane_holder, LiveTabCwd};
use codeterm_util::{safe_tree, worktree_cleanup::git};

pub struct Residue {
    pub path: PathBuf,
    pub refusal: Option<String>,
}

pub fn worktree_root(canonical: &Path) -> Result<PathBuf, String> {
    let name = canonical
        .file_name()
        .ok_or("canonical checkout has no directory name")?;
    let parent = canonical
        .parent()
        .ok_or("canonical checkout has no parent")?;
    Ok(parent.join(format!("{}-worktrees", name.to_string_lossy())))
}

pub fn residue_decision(
    registered: bool,
    canonical: bool,
    linked: bool,
    artifacts_only: bool,
) -> Option<&'static str> {
    if canonical {
        Some("canonical checkout protected")
    } else if registered {
        Some("registered worktree protected")
    } else if linked {
        Some("reparse point or symlink protected")
    } else if !artifacts_only {
        Some("source, .git, or unknown content protected")
    } else {
        None
    }
}

struct RepositorySnapshot {
    worktrees: Vec<String>,
    tracked: Vec<PathBuf>,
}

fn snapshot(canonical: &str) -> Result<RepositorySnapshot, String> {
    let list = git(canonical, &["worktree", "list", "--porcelain", "-z"])?;
    let worktrees = list
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(|name| {
            std::str::from_utf8(name)
                .map(str::to_string)
                .map_err(|_| "non-UTF-8 worktree path".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    snapshot_with_worktrees(canonical, worktrees)
}

fn snapshot_with_worktrees(
    canonical: &str,
    worktrees: Vec<String>,
) -> Result<RepositorySnapshot, String> {
    let tracked = git(canonical, &["ls-files", "-z"])?;
    let tracked = tracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| {
            std::str::from_utf8(name)
                .map(PathBuf::from)
                .map_err(|_| "non-UTF-8 tracked path".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RepositorySnapshot { worktrees, tracked })
}

fn artifacts_only(path: &Path) -> Result<bool, String> {
    for entry in std::fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let metadata =
            std::fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if matches!(name.to_str(), Some("src-tauri" | "cli"))
            && metadata.is_dir()
            && !safe_tree::is_link(&metadata)
        {
            for child in std::fs::read_dir(entry.path()).map_err(|error| error.to_string())? {
                let child = child.map_err(|error| error.to_string())?;
                let metadata =
                    std::fs::symlink_metadata(child.path()).map_err(|error| error.to_string())?;
                if child.file_name() != "target"
                    || !(metadata.is_dir() || safe_tree::is_link(&metadata))
                {
                    return Ok(false);
                }
            }
            continue;
        }
        let allowed = if metadata.is_dir() || safe_tree::is_link(&metadata) {
            matches!(
                name.to_str(),
                Some("node_modules" | "target" | "dist" | "build" | ".cache")
            )
        } else {
            matches!(name.to_str(), Some("README" | "README.md"))
        };
        if !allowed {
            return Ok(false);
        }
    }
    let mut pending = vec![path.to_path_buf()];
    let mut entries = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            entries += 1;
            if entries > 1_000_000 {
                return Err(format!(
                    "residue inspection budget exceeded: '{}'",
                    path.display()
                ));
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(".git")
            {
                return Ok(false);
            }
            let metadata =
                std::fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.is_dir() && !safe_tree::is_link(&metadata) {
                pending.push(entry.path());
            }
        }
    }
    Ok(true)
}

fn inspect(
    canonical: &str,
    root: &Path,
    path: &Path,
    tabs: &[LiveTabCwd],
    snapshot: &RepositorySnapshot,
) -> Result<Option<String>, String> {
    if path.parent() != Some(root) {
        return Err(format!(
            "residue outside worktree root: '{}'",
            path.display()
        ));
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let is_registered = snapshot
        .worktrees
        .iter()
        .any(|name| crate::worker_lane::same_path(name, &path.to_string_lossy()));
    let linked = safe_tree::is_link(&metadata);
    if let Some(reason) = residue_decision(
        is_registered,
        crate::worker_lane::same_path(canonical, &path.to_string_lossy()),
        linked,
        true,
    ) {
        return Ok(Some(reason.into()));
    }
    let tracked_source = snapshot
        .tracked
        .iter()
        .filter(|name| !matches!(name.to_str(), Some("README" | "README.md")))
        .any(|name| std::fs::symlink_metadata(path.join(name)).is_ok());
    if let Some(reason) = residue_decision(
        is_registered,
        crate::worker_lane::same_path(canonical, &path.to_string_lossy()),
        linked,
        !linked && !is_registered && !tracked_source && metadata.is_dir() && artifacts_only(path)?,
    ) {
        return Ok(Some(reason.into()));
    }
    Ok(find_lane_holder(&path.to_string_lossy(), tabs)
        .map(|tab| format!("held by tab {} ({})", tab.id, tab.title)))
}

pub fn inventory(canonical: &str, tabs: &[LiveTabCwd]) -> Result<Vec<Residue>, String> {
    inventory_using(canonical, tabs, None)
}

pub fn inventory_with_worktrees(
    canonical: &str,
    tabs: &[LiveTabCwd],
    worktrees: &[String],
) -> Result<Vec<Residue>, String> {
    inventory_using(canonical, tabs, Some(worktrees))
}

fn inventory_using(
    canonical: &str,
    tabs: &[LiveTabCwd],
    worktrees: Option<&[String]>,
) -> Result<Vec<Residue>, String> {
    let root = worktree_root(Path::new(canonical))?;
    let root = match std::fs::canonicalize(&root) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", root.display())),
    };
    let directories = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", root.display())),
    };
    let snapshot = match worktrees {
        Some(worktrees) => snapshot_with_worktrees(canonical, worktrees.to_vec())?,
        None => snapshot(canonical)?,
    };
    let mut rows = Vec::new();
    for entry in directories {
        let path = entry.map_err(|error| error.to_string())?.path();
        let refusal =
            inspect(canonical, &root, &path, tabs, &snapshot).unwrap_or_else(|reason| Some(reason));
        rows.push(Residue { path, refusal });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}

pub fn remove(canonical: &str, path: &Path, tabs: &[LiveTabCwd]) -> Result<(), String> {
    let root = worktree_root(Path::new(canonical))?;
    let root = std::fs::canonicalize(&root).map_err(|error| error.to_string())?;
    let parent = path.parent().ok_or("residue has no parent")?;
    let parent = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
    if parent != root {
        return Err(format!(
            "residue outside worktree root: '{}'",
            path.display()
        ));
    }
    let path = root.join(path.file_name().ok_or("residue has no directory name")?);
    let snapshot = snapshot(canonical)?;
    if let Some(reason) = inspect(canonical, &root, &path, tabs, &snapshot)? {
        return Err(format!("{}: {reason}", path.display()));
    }
    safe_tree::remove(&path)
}

#[cfg(test)]
mod tests;
