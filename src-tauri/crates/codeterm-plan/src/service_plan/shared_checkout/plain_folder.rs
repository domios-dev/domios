//! A project folder without git history (no repository, or a repository with
//! no commit) is a valid canonical project. Shared Workers edit it in place and
//! content hashes of their declared edit files stand in for a commit.

use super::report::SharedReportError;
use crate::plan::{TaskFiles, WorkerPlacement};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub const NO_GIT_HISTORY: &str = "isolated lanes and the canonical writer need a git repository \
     with at least one commit, and this project folder has none. Domios never runs git init or \
     commits for you: declare disjoint edit files and dispatch with shared placement";

const MAX_HASHED_FILES: usize = 4096;

/// Path relative to the project folder -> `sha256:<hex>` of its content.
pub type FileHashes = BTreeMap<String, String>;
/// Changed declared path -> its new content hash, or `None` when it was removed.
pub type ChangedFileHashes = BTreeMap<String, Option<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectHistory {
    Git,
    /// No repository, or a repository whose HEAD names no commit yet.
    NoGit,
}

/// What a HEAD probe saw in a folder that carries a `.git` marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadProbe {
    Commit,
    Unborn,
    Failed(String),
}

/// A folder without any `.git` marker is decided without spawning git; an
/// unreadable repository stays an inspection failure, never a plain folder.
pub fn classify_history(
    has_git_marker: bool,
    probe: impl FnOnce() -> HeadProbe,
) -> Result<ProjectHistory, String> {
    if !has_git_marker {
        return Ok(ProjectHistory::NoGit);
    }
    match probe() {
        HeadProbe::Commit => Ok(ProjectHistory::Git),
        HeadProbe::Unborn => Ok(ProjectHistory::NoGit),
        HeadProbe::Failed(reason) => Err(reason),
    }
}

pub fn project_history(checkout: &str) -> Result<ProjectHistory, String> {
    let has_marker = Path::new(checkout)
        .ancestors()
        .any(|dir| dir.join(".git").exists());
    classify_history(has_marker, || probe_head(checkout))
}

fn probe_head(checkout: &str) -> HeadProbe {
    let run = |args: &[&str]| {
        let mut command = Command::new("git");
        codeterm_util::process_util::no_window_std(&mut command);
        command
            .arg("-C")
            .arg(checkout)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
    };
    let head = match run(&["rev-parse", "--verify", "-q", "HEAD^{commit}"]) {
        Ok(output) => output,
        Err(error) => return HeadProbe::Failed(format!("git could not run: {error}")),
    };
    if head.status.success() {
        return HeadProbe::Commit;
    }
    // `--verify -q` exits 1 silently for an unborn HEAD; `--git-dir` proves the
    // repository itself is readable, so corruption is not mistaken for no history.
    if head.status.code() == Some(1)
        && run(&["rev-parse", "--git-dir"]).is_ok_and(|output| output.status.success())
    {
        return HeadProbe::Unborn;
    }
    HeadProbe::Failed(format!(
        "git HEAD inspection failed in '{checkout}' (status {}): {}",
        head.status,
        String::from_utf8_lossy(&head.stderr).trim()
    ))
}

/// Lanes and the canonical writer need git; automatic placement never picks a
/// lane here, so every lane reason becomes a refusal naming the cause.
pub fn placement_without_git(
    requested: Option<&str>,
    chosen: impl FnOnce() -> Result<(WorkerPlacement, String), String>,
) -> Result<(WorkerPlacement, String), String> {
    match requested {
        Some("lane" | "isolated_lane") => {
            return Err(format!("lane placement refused: {NO_GIT_HISTORY}"))
        }
        Some("canonical_writer") => {
            return Err(format!(
                "canonical writer placement refused: {NO_GIT_HISTORY}"
            ))
        }
        _ => {}
    }
    match chosen()? {
        (WorkerPlacement::IsolatedLane, reason) => Err(format!(
            "shared placement refused: {reason}; {NO_GIT_HISTORY}"
        )),
        (placement, _) => Ok((
            placement,
            "disjoint declared edit files in a project folder without git".into(),
        )),
    }
}

/// Hashes every regular file under the declared edit paths; links are never followed.
pub fn hash_declared_files(checkout: &str, files: &TaskFiles) -> Result<FileHashes, String> {
    let mut hashes = FileHashes::new();
    for edit in files.shared_edit_paths()? {
        hash_path(Path::new(checkout), &edit, &mut hashes)?;
    }
    Ok(hashes)
}

fn hash_path(root: &Path, relative: &str, hashes: &mut FileHashes) -> Result<(), String> {
    let path = root.join(relative);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot inspect declared path '{relative}': {error}"
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        let mut names = std::fs::read_dir(&path)
            .map_err(|error| format!("cannot list declared path '{relative}': {error}"))?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot list declared path '{relative}': {error}"))?;
        names.sort();
        for name in names {
            hash_path(root, &format!("{relative}/{name}"), hashes)?;
        }
        return Ok(());
    }
    if hashes.len() >= MAX_HASHED_FILES {
        return Err(format!(
            "declared edit paths hold more than {MAX_HASHED_FILES} files; declare narrower edit paths"
        ));
    }
    let mut file = std::fs::File::open(&path)
        .map_err(|error| format!("cannot read declared file '{relative}': {error}"))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|error| format!("cannot read declared file '{relative}': {error}"))?;
    hashes.insert(
        relative.to_string(),
        format!("sha256:{:x}", hasher.finalize()),
    );
    Ok(())
}

pub fn changed_file_hashes(start: &FileHashes, current: &FileHashes) -> ChangedFileHashes {
    start
        .keys()
        .chain(current.keys())
        .filter(|path| start.get(*path) != current.get(*path))
        .map(|path| (path.clone(), current.get(path).cloned()))
        .collect()
}

/// The report decision for a shared attempt without git: no commit SHA, and at
/// least one declared file must differ from its dispatch-time content.
pub(super) fn validate_report_without_git(
    files: &TaskFiles,
    start: &FileHashes,
    current: &FileHashes,
    sha: Option<&str>,
) -> Result<ChangedFileHashes, SharedReportError> {
    if sha.is_some_and(|sha| !sha.trim().is_empty()) {
        return Err(SharedReportError::Rejected(
            "this project folder has no git history, so a shared report names no commit: \
             report without --sha. Domios records content hashes of your declared edit files \
             as the evidence"
                .into(),
        ));
    }
    let changed: ChangedFileHashes = changed_file_hashes(start, current)
        .into_iter()
        .filter(|(path, _)| files.owns_edit_path(path))
        .collect();
    if changed.is_empty() {
        return Err(SharedReportError::Rejected(
            "no declared edit file changed since dispatch; edit your declared files in place \
             before reporting in_review"
                .into(),
        ));
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
