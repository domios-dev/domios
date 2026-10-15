use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::ServiceContext;
use crate::process_util;
use crate::service::plan::{PlanStore, TaskLaneUpdate};

/// One live tab as the removal preflight sees it: the shell cwd decides
/// ownership, the title and id are what a refusal has to name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTabCwd {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneRemovalRequest {
    pub(crate) worktree_path: String,
    pub(crate) branch: Option<String>,
    pub(crate) force: bool,
    /// Canonical checkout the lane was branched from. Derived from the lane's
    /// own git common dir when the caller does not name one.
    pub(crate) canonical_cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneRemovalOutcome {
    pub(crate) canonical_cwd: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: Option<String>,
    pub(crate) worktree_already_removed: bool,
    pub(crate) branch_already_removed: bool,
}

#[derive(Debug)]
pub enum CleanupStatusDoneError {
    Refused(String),
    Verification(String),
}

const MAX_LANE_WALK_ENTRIES: usize = 100_000;
const MAX_LANE_WALK_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkRemovalKind {
    Directory,
    File,
}

fn link_removal_kind(
    is_symlink: bool,
    metadata_is_directory: bool,
    windows_directory_attribute: bool,
) -> Option<LinkRemovalKind> {
    if !is_symlink {
        None
    } else if metadata_is_directory || windows_directory_attribute {
        Some(LinkRemovalKind::Directory)
    } else {
        Some(LinkRemovalKind::File)
    }
}

pub(crate) fn remove_lane_links(path: &Path) -> Result<(), String> {
    remove_lane_links_except(path, HashSet::new())
}

fn remove_lane_links_except(path: &Path, preserved: HashSet<PathBuf>) -> Result<(), String> {
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("lane-link-cleanup".into())
        .spawn(move || remove_lane_links_blocking(&path, &preserved))
        .map_err(|error| format!("cannot start lane link scan: {error}"))?
        .join()
        .map_err(|_| "lane link scan thread panicked".to_string())?
}

fn refuse_linked_lane_path(path: &Path) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot inspect lane path '{}': {error}", path.display()))?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "lane removal refused: path component '{}' is a symlink or reparse point",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!(
                    "cannot inspect lane path component '{}': {error}",
                    current.display()
                ))
            }
        }
    }
    Ok(())
}

fn remove_lane_links_blocking(path: &Path, preserved: &HashSet<PathBuf>) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot inspect lane path '{}': {error}",
                path.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return remove_link(path, &metadata);
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_LANE_WALK_DEPTH {
            return Err(format!(
                "lane link scan exceeded depth at '{}'",
                directory.display()
            ));
        }
        let entries = std::fs::read_dir(&directory).map_err(|error| {
            format!(
                "cannot scan lane directory '{}': {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            visited += 1;
            if visited > MAX_LANE_WALK_ENTRIES {
                return Err(format!(
                    "lane link scan exceeded {MAX_LANE_WALK_ENTRIES} entries at '{}'",
                    directory.display()
                ));
            }
            let entry = entry.map_err(|error| {
                format!(
                    "cannot inspect lane entry in '{}': {error}",
                    directory.display()
                )
            })?;
            let child = entry.path();
            let metadata = std::fs::symlink_metadata(&child).map_err(|error| {
                format!("cannot inspect lane entry '{}': {error}", child.display())
            })?;
            if metadata.file_type().is_symlink() {
                if !preserved.contains(&child) {
                    remove_link(&child, &metadata)?;
                }
            } else if metadata.is_dir() {
                pending.push((child, depth + 1));
            }
        }
    }
    Ok(())
}

fn remove_link(path: &Path, metadata: &std::fs::Metadata) -> Result<(), String> {
    let kind = link_removal_kind(
        metadata.file_type().is_symlink(),
        metadata.is_dir(),
        windows_directory_attribute(metadata),
    )
    .ok_or_else(|| {
        format!(
            "lane entry '{}' is not a symlink or reparse point",
            path.display()
        )
    })?;
    clear_readonly_link_attribute(path, metadata)?;
    let result = match kind {
        LinkRemovalKind::Directory => std::fs::remove_dir(path),
        LinkRemovalKind::File => std::fs::remove_file(path),
    };
    result.map_err(|error| {
        format!(
            "cannot unlink lane reparse point or symlink '{}': {error}",
            path.display()
        )
    })
}

#[cfg(windows)]
fn windows_directory_attribute(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
        != 0
}

#[cfg(not(windows))]
fn windows_directory_attribute(_: &std::fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn clear_readonly_link_attribute(path: &Path, metadata: &std::fs::Metadata) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, SetFileInformationByHandle, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
    };

    let attributes = metadata.file_attributes();
    if attributes & FILE_ATTRIBUTE_READONLY == 0 {
        return Ok(());
    }
    let handle = std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot open lane reparse point '{}' to clear read-only: {error}",
                path.display()
            )
        })?;
    let info = FILE_BASIC_INFO {
        FileAttributes: attributes & !FILE_ATTRIBUTE_READONLY,
        ..Default::default()
    };
    let succeeded = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle() as _,
            FileBasicInfo,
            (&info as *const FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(format!(
            "cannot clear read-only attribute on lane reparse point '{}': {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_readonly_link_attribute(_: &Path, _: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}

pub(crate) fn remove_lane_residue(path: &Path) -> Result<(), String> {
    remove_lane_links(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => remove_link(path, &metadata)?,
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path).map_err(|error| {
            format!(
                "cannot remove lane leftovers at '{}': {error}",
                path.display()
            )
        })?,
        Ok(_) => std::fs::remove_file(path).map_err(|error| {
            format!("cannot remove lane leftover '{}': {error}", path.display())
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot inspect lane leftovers '{}': {error}",
                path.display()
            ))
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(format!("lane leftovers remain at '{}'", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "cannot verify lane leftovers at '{}': {error}",
            path.display()
        )),
    }
}

pub(crate) fn remove_lane_worktree(
    canonical_cwd: &str,
    worktree_path: &str,
    force: bool,
) -> Result<(), String> {
    let path = Path::new(worktree_path);
    refuse_linked_lane_path(path)?;
    if force {
        remove_lane_links(path)?;
    } else {
        let index = git_output(worktree_path, ["ls-files", "--stage", "-z"])?;
        if !index.status.success() {
            return Err(format!(
                "cannot inspect tracked lane links: {}",
                String::from_utf8_lossy(&index.stderr).trim()
            ));
        }
        let mut preserved = HashSet::new();
        for entry in index
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| entry.starts_with(b"120000 "))
        {
            let name = entry
                .splitn(2, |byte| *byte == b'\t')
                .nth(1)
                .ok_or_else(|| "cannot parse tracked lane link".to_string())?;
            let name = std::str::from_utf8(name)
                .map_err(|_| "tracked lane link has a non-UTF-8 name".to_string())?;
            preserved.insert(path.join(name));
        }
        remove_lane_links_except(path, preserved)?;
    }
    if force {
        run_git(
            canonical_cwd,
            ["worktree", "remove", "--force", worktree_path],
        )
        .map_err(|error| format!("lane removal failed for '{worktree_path}': {error}"))?;
    } else {
        run_git(canonical_cwd, ["worktree", "remove", worktree_path])
            .map_err(|error| format!("lane removal failed for '{worktree_path}': {error}"))?;
    }
    remove_lane_residue(path)
}

pub(crate) fn recorded_lane_remnant(
    worktree_path: Option<&str>,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> Result<Option<String>, String> {
    let worktree_path = non_empty(worktree_path);
    let branch = non_empty(branch);
    if worktree_path.is_none() && branch.is_none() {
        return Ok(None);
    }
    if let Some(path) = worktree_path {
        match std::fs::symlink_metadata(strip_verbatim(&path)) {
            Ok(_) => return Ok(Some(format!("directory '{path}' still exists"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect recorded lane '{path}': {error}")),
        }
        if worktree_registered(canonical_cwd, &path)? {
            return Ok(Some(format!(
                "Git worktree registration for '{path}' still exists"
            )));
        }
    }
    if let Some(branch) = branch {
        if branch_exists(canonical_cwd, &branch)? && !branch_is_ancestor(canonical_cwd, &branch)? {
            return Ok(Some(format!(
                "branch '{branch}' still has commits not merged into the target checkout"
            )));
        }
    }
    Ok(None)
}

fn path_components(path: &str) -> Vec<String> {
    comparable_path(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn comparable_path(path: &str) -> String {
    let separators_normalized = strip_verbatim(path).replace('\\', "/");
    codeterm_paths::normalize_cwd(&separators_normalized).replace('\\', "/")
}

/// A tab holds the lane when its shell cwd is the worktree root or anything
/// under it. Compared component-wise so a sibling lane whose name merely starts
/// with the target's cannot read as inside it.
pub(crate) fn find_lane_holder<'a>(
    worktree_path: &str,
    live_tabs: &'a [LiveTabCwd],
) -> Option<&'a LiveTabCwd> {
    let target = path_components(worktree_path);
    if target.is_empty() {
        return None;
    }
    live_tabs.iter().find(|tab| {
        let candidate = path_components(&tab.cwd);
        candidate.len() >= target.len() && candidate[..target.len()] == target[..]
    })
}

pub(crate) fn holder_refusal(worktree_path: &str, holder: &LiveTabCwd) -> String {
    format!(
        "✗ lane removal refused: tab '{title}' ({id}) is live with its shell in '{cwd}', \
inside the lane worktree '{worktree_path}', so that directory is still in use. Nothing was removed.\n\
close that tab first, then run the removal again:\n  codeterm tab close --tab {id}",
        title = holder.title,
        id = holder.id,
        cwd = holder.cwd,
    )
}

pub(crate) fn remove_lane(
    request: &LaneRemovalRequest,
    live_tabs: &[LiveTabCwd],
) -> Result<LaneRemovalOutcome, String> {
    let worktree = strip_verbatim(request.worktree_path.trim());
    let worktree = worktree.as_str();
    if worktree.is_empty() {
        return Err("lane removal requires a worktree path".into());
    }
    if let Some(holder) = find_lane_holder(worktree, live_tabs) {
        return Err(holder_refusal(worktree, holder));
    }
    refuse_linked_lane_path(Path::new(worktree))?;
    let worktree_exists = match std::fs::symlink_metadata(strip_verbatim(worktree)) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "cannot inspect lane worktree '{worktree}': {error}"
            ))
        }
    };
    let canonical = match request
        .canonical_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => codeterm_paths::canonical_cwd(value),
        None if worktree_exists => canonical_checkout_for(worktree)?,
        None => {
            return Err(format!(
                "lane worktree '{worktree}' is already absent; pass --project to check its branch"
            ))
        }
    };
    if super::worker_lane::same_path(&canonical, worktree) {
        return Err(format!(
            "✗ lane removal refused: '{worktree}' is the canonical checkout, not a lane worktree. Nothing was removed."
        ));
    }
    let registered = worktree_registered(&canonical, worktree)?;
    let branch = request
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let branch_exists = branch
        .map(|branch| branch_exists(&canonical, branch))
        .transpose()?
        .unwrap_or(false);
    if let Some(branch) = branch.filter(|_| branch_exists) {
        if !branch_is_ancestor(&canonical, branch)? {
            return Err(format!(
                "lane removal refused: branch '{branch}' has commits not merged into target checkout '{canonical}'; nothing was removed. Merge the needed commits, then rerun: {}",
                lane_remove_command(worktree, Some(branch), &canonical)
            ));
        }
    }
    if worktree_exists {
        if !registered {
            return Err(format!(
                "lane path '{worktree}' still exists but is not registered as a Git worktree; nothing was removed"
            ));
        }
        remove_lane_worktree(&canonical, worktree, request.force)?;
    } else if registered {
        remove_stale_worktree_registration(&canonical, worktree).map_err(|error| {
            format!("stale lane registration for '{worktree}' could not be removed: {error}")
        })?;
        if worktree_registered(&canonical, worktree)? {
            return Err(format!(
                "stale lane registration for '{worktree}' remains after targeted cleanup"
            ));
        }
    }
    if let Some(branch) = branch {
        if branch_exists {
            run_git(&canonical, ["branch", "-d", branch]).map_err(|error| {
                format!(
                    "lane worktree '{worktree}' was removed but branch '{branch}' was not: {error}"
                )
            })?;
        }
    }
    let branch_already_removed = branch.is_some() && !branch_exists;
    Ok(LaneRemovalOutcome {
        canonical_cwd: canonical,
        worktree_path: worktree.to_string(),
        branch: branch.map(str::to_string),
        worktree_already_removed: !worktree_exists,
        branch_already_removed,
    })
}

/// The checkout a linked worktree belongs to: its git common dir is that
/// checkout's `.git`, so the parent is the canonical working tree.
fn canonical_checkout_for(worktree_path: &str) -> Result<String, String> {
    let common = git_value(
        &strip_verbatim(worktree_path),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Path::new(&common)
        .parent()
        .map(|path| codeterm_paths::canonical_cwd(&path.to_string_lossy()))
        .ok_or_else(|| {
            format!("cannot resolve the canonical checkout owning lane '{worktree_path}'")
        })
}

fn git_command<'a>(cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Command {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command.current_dir(strip_verbatim(cwd)).args(args);
    command
}

fn git_output<'a>(
    cwd: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<std::process::Output, String> {
    git_command(cwd, args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| format!("git could not be run in '{cwd}': {error}"))
}

fn run_git<'a>(cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let output = git_command(cwd, args)
        .output()
        .map_err(|error| format!("git could not be run in '{cwd}': {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn git_value<'a>(cwd: &str, args: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect lane '{cwd}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn worktree_registered(canonical_cwd: &str, worktree_path: &str) -> Result<bool, String> {
    let output = git_output(canonical_cwd, ["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect Git worktree registrations in '{}': {}",
            canonical_cwd,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let expected = comparable_path(worktree_path);
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|path| comparable_path(path) == expected))
}

fn remove_stale_worktree_registration(
    canonical_cwd: &str,
    worktree_path: &str,
) -> Result<(), String> {
    let worktrees = git_value(
        canonical_cwd,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "worktrees",
        ],
    )?;
    let worktrees = PathBuf::from(strip_verbatim(&worktrees));
    let expected = format!(
        "{}/.git",
        comparable_path(worktree_path).trim_end_matches('/')
    );
    let entries = std::fs::read_dir(&worktrees).map_err(|error| {
        format!(
            "cannot read Git worktree metadata '{}': {error}",
            worktrees.display()
        )
    })?;
    let mut matched = None;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot inspect Git worktree metadata: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("cannot inspect Git worktree metadata: {error}"))?
            .is_dir()
        {
            continue;
        }
        let registration = entry.path();
        let gitdir_file = registration.join("gitdir");
        let gitdir = match std::fs::read_to_string(&gitdir_file) {
            Ok(gitdir) => gitdir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot read '{}': {error}", gitdir_file.display())),
        };
        if comparable_path(gitdir.trim()) != expected {
            continue;
        }
        if matched.is_some() {
            return Err(format!("multiple registrations match '{worktree_path}'"));
        }
        if registration.join("locked").exists() {
            return Err(format!(
                "the stale registration '{}' is locked",
                registration.display()
            ));
        }
        matched = Some(registration);
    }
    let registration = matched.ok_or_else(|| {
        format!("Git registration for '{worktree_path}' has no matching metadata directory")
    })?;
    std::fs::remove_dir_all(&registration)
        .map_err(|error| format!("cannot remove '{}': {error}", registration.display()))
}

fn branch_exists(canonical_cwd: &str, branch: &str) -> Result<bool, String> {
    let reference = format!("refs/heads/{branch}");
    let output = git_output(
        canonical_cwd,
        ["show-ref", "--verify", "--quiet", &reference],
    )?;
    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(format!(
            "cannot inspect branch '{branch}' in '{canonical_cwd}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn branch_is_ancestor(canonical_cwd: &str, branch: &str) -> Result<bool, String> {
    let output = git_output(
        canonical_cwd,
        ["merge-base", "--is-ancestor", branch, "HEAD"],
    )?;
    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(format!(
            "cannot verify whether branch '{branch}' is merged into '{canonical_cwd}': {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn strip_verbatim(path: &str) -> String {
    codeterm_paths::strip_verbatim_prefix(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn lane_remove_command(
    worktree_path: &str,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> String {
    let mut command = format!(
        "codeterm plan lane-remove {}",
        shell_quote(&strip_verbatim(worktree_path))
    );
    if let Some(branch) = branch {
        command.push_str(&format!(" --branch {}", shell_quote(branch)));
    }
    command.push_str(&format!(
        " --project {}",
        shell_quote(&strip_verbatim(canonical_cwd))
    ));
    command
}

pub(crate) fn lane_release_command(
    worktree_path: Option<&str>,
    branch: Option<&str>,
    canonical_cwd: &str,
) -> Option<String> {
    if let Some(worktree_path) = worktree_path {
        Some(lane_remove_command(worktree_path, branch, canonical_cwd))
    } else {
        branch.map(|branch| {
            format!(
                "git -C {} branch -d {}",
                shell_quote(&strip_verbatim(canonical_cwd)),
                shell_quote(branch)
            )
        })
    }
}

pub fn clear_removed_task_lane_references(
    store: &PlanStore,
    outcome: &LaneRemovalOutcome,
) -> Result<(), String> {
    for plan in store.list_full() {
        for task in &plan.tasks {
            let Some(path) = task.lane.worktree_path.as_deref() else {
                continue;
            };
            if comparable_path(path) != comparable_path(&outcome.worktree_path) {
                continue;
            }
            let branch_removed = task
                .lane
                .branch
                .as_deref()
                .map(|task_branch| branch_exists(&outcome.canonical_cwd, task_branch))
                .transpose()?
                .is_none_or(|exists| !exists);
            let update = TaskLaneUpdate {
                tab_id: Some(None),
                workspace_id: Some(None),
                workspace_name: Some(None),
                worktree_path: Some(None),
                branch: branch_removed.then_some(None),
                cleanup_status: branch_removed.then(|| Some("done".into())),
                ..Default::default()
            };
            store
                .update_task_lane(&plan.id, &task.id, update, unix_time_millis())
                .map_err(|error| {
                    format!(
                        "lane was removed but task '{}' in plan '{}' still has lane references: {error}",
                        task.id, plan.id
                    )
                })?;
        }
    }
    Ok(())
}

pub fn project_for_recorded_lane(store: &PlanStore, worktree_path: &str) -> Option<String> {
    store.list_full().into_iter().find_map(|plan| {
        plan.tasks
            .iter()
            .any(|task| {
                task.lane
                    .worktree_path
                    .as_deref()
                    .is_some_and(|path| comparable_path(path) == comparable_path(worktree_path))
            })
            .then(|| {
                if plan.canonical_project_path.trim().is_empty() {
                    plan.project_path
                } else {
                    plan.canonical_project_path
                }
            })
    })
}

pub fn remove_lane_and_clear_task_references(
    store: &PlanStore,
    repo_watch: &crate::repowatch::RepoWatchRegistryHandle,
    request: &LaneRemovalRequest,
    live_tabs: &[LiveTabCwd],
) -> Result<LaneRemovalOutcome, String> {
    let mut request = request.clone();
    if request.canonical_cwd.as_deref().is_none_or(str::is_empty) {
        request.canonical_cwd = project_for_recorded_lane(store, &request.worktree_path);
    }
    let outcome = remove_lane(&request, live_tabs)?;
    finalize_lane_removal(store, repo_watch, &outcome)?;
    Ok(outcome)
}

pub fn finalize_lane_removal(
    store: &PlanStore,
    repo_watch: &crate::repowatch::RepoWatchRegistryHandle,
    outcome: &LaneRemovalOutcome,
) -> Result<(), String> {
    let attempt_ids: Vec<String> = store
        .list_full()
        .into_iter()
        .flat_map(|plan| plan.tasks)
        .flat_map(|task| task.execution_attempts)
        .filter(|attempt| {
            attempt.worktree_path.as_deref().is_some_and(|path| {
                comparable_path(path) == comparable_path(&outcome.worktree_path)
            })
        })
        .map(|attempt| attempt.id)
        .collect();
    repo_watch.release_lane_watch(&outcome.worktree_path);
    for attempt_id in attempt_ids {
        super::lane_stats::delete_attempt_checkpoint_refs(Path::new(&outcome.canonical_cwd), &attempt_id)
            .map_err(|error| format!("lane worktree '{}' was removed but attempt '{attempt_id}' checkpoint refs remain: {error}", outcome.worktree_path))?;
    }
    clear_removed_task_lane_references(store, outcome)
}

pub(crate) fn clear_vanished_lane_on_cleanup_done(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    update: Option<TaskLaneUpdate>,
) -> Result<Option<TaskLaneUpdate>, CleanupStatusDoneError> {
    let Some(mut update) = update else {
        return Ok(None);
    };
    let is_done = update
        .cleanup_status
        .as_ref()
        .and_then(Option::as_deref)
        .is_some_and(|status| status.eq_ignore_ascii_case("done"));
    if !is_done {
        return Ok(Some(update));
    }
    let Some(plan) = store.get(plan_id) else {
        return Ok(Some(update));
    };
    let Some(task) = plan.tasks.iter().find(|task| task.id == task_id) else {
        return Ok(Some(update));
    };
    let worktree_path = task.lane.worktree_path.as_deref();
    let branch = task.lane.branch.as_deref();
    if worktree_path.is_none() && branch.is_none() {
        return Ok(Some(update));
    }
    let canonical_cwd = if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    let remnant = recorded_lane_remnant(worktree_path, branch, canonical_cwd)
        .map_err(CleanupStatusDoneError::Verification)?;
    if let Some(remnant) = remnant {
        let release = lane_release_command(worktree_path, branch, canonical_cwd)
            .unwrap_or_else(|| "codeterm plan lane-remove <worktree>".into());
        return Err(CleanupStatusDoneError::Refused(format!(
            "cleanup is not complete: {remnant}. Release the recorded lane with: {release}"
        )));
    }
    update.tab_id = Some(None);
    update.workspace_id = Some(None);
    update.workspace_name = Some(None);
    update.worktree_path = Some(None);
    update.branch = Some(None);
    Ok(Some(update))
}

pub fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl ServiceContext {
    pub fn live_tab_cwds(&self) -> Vec<LiveTabCwd> {
        self.mux
            .read()
            .all_tabs()
            .filter_map(|tab| {
                let cwd = tab.cwd.clone()?;
                (!cwd.trim().is_empty()).then(|| LiveTabCwd {
                    id: tab.id.clone(),
                    title: tab.title.clone(),
                    cwd,
                })
            })
            .collect()
    }

    pub fn remove_owned_lane(
        &self,
        request: &LaneRemovalRequest,
    ) -> Result<LaneRemovalOutcome, String> {
        remove_lane_and_clear_task_references(
            &self.plan_store,
            self.ext::<crate::repowatch::RepoWatchRegistryHandle>(),
            request,
            &self.live_tab_cwds(),
        )
    }
}

#[cfg(test)]
mod tests;
