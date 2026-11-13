use std::path::Path;
use std::process::{Command, Output};

pub fn git(cwd: &str, args: &[&str]) -> Result<Output, String> {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| format!("git at '{cwd}': {error}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "git {} at '{cwd}': {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub fn remove(canonical: &str, worktree: &str, force: bool) -> Result<(), String> {
    let path = Path::new(worktree);
    crate::safe_tree::refuse_linked_ancestors(path)?;
    let canonical_path = std::fs::canonicalize(canonical).map_err(|error| error.to_string())?;
    let lane_path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let (_, common) =
        crate::git_checkout::paths(Path::new(canonical)).map_err(|error| error.to_string())?;
    let repository = common
        .parent()
        .ok_or("repository has no canonical checkout")?;
    let repository = std::fs::canonicalize(repository).map_err(|error| error.to_string())?;
    if lane_path == canonical_path
        || canonical_path.starts_with(&lane_path)
        || repository.starts_with(&lane_path)
    {
        return Err(format!("refusing canonical checkout removal: '{worktree}'"));
    }
    let list = git(canonical, &["worktree", "list", "--porcelain", "-z"])?;
    let registered = list
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .filter_map(|name| std::str::from_utf8(name).ok())
        .any(|name| std::fs::canonicalize(name).is_ok_and(|name| name == lane_path));
    if !registered {
        return Err(format!("unregistered worktree: '{worktree}'"));
    }
    if !force {
        let status = git(
            worktree,
            &["status", "--porcelain", "--untracked-files=all"],
        )?;
        if !status.stdout.is_empty() {
            return Err(format!("dirty lane protected: '{worktree}'"));
        }
    }
    for record in list
        .stdout
        .split(|byte| *byte == 0)
        .collect::<Vec<_>>()
        .split(|field| field.is_empty())
    {
        let owns = record
            .iter()
            .filter_map(|field| field.strip_prefix(b"worktree "))
            .filter_map(|name| std::str::from_utf8(name).ok())
            .any(|name| std::fs::canonicalize(name).is_ok_and(|name| name == lane_path));
        if owns && record.iter().any(|field| field.starts_with(b"locked")) {
            return Err(format!("locked worktree protected: '{worktree}'"));
        }
    }
    let (registration, lane_common) =
        crate::git_checkout::paths(Path::new(worktree)).map_err(|error| error.to_string())?;
    if lane_common != common {
        return Err(format!(
            "worktree belongs to a different repository: '{worktree}'"
        ));
    }
    let registrations =
        std::fs::canonicalize(common.join("worktrees")).map_err(|error| error.to_string())?;
    if registration.parent() != Some(registrations.as_path()) {
        return Err(format!(
            "worktree registration is outside its repository: '{}'",
            registration.display()
        ));
    }
    crate::safe_tree::refuse_linked_ancestors(&registration)?;
    crate::safe_tree::remove(path)?;
    if let Err(error) = git(canonical, &["worktree", "remove", "--force", worktree]) {
        if registration.join("locked").exists() {
            return Err(error);
        }
        crate::safe_tree::remove(&registration).map_err(|reason| format!("{error}; {reason}"))?;
    }
    let remaining = git(canonical, &["worktree", "list", "--porcelain", "-z"])?;
    let expected = comparable(worktree);
    if remaining
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .filter_map(|name| std::str::from_utf8(name).ok())
        .any(|name| comparable(name) == expected)
    {
        return Err(format!("worktree registration remains for '{worktree}'"));
    }
    Ok(())
}

/// The one directory Domios provisions lane worktrees in: `<checkout>-worktrees`.
pub fn lane_worktree_root(canonical: &Path) -> Result<std::path::PathBuf, String> {
    let name = canonical
        .file_name()
        .ok_or("canonical checkout has no directory name")?;
    let parent = canonical
        .parent()
        .ok_or("canonical checkout has no parent")?;
    Ok(parent.join(format!("{}-worktrees", name.to_string_lossy())))
}

fn resolved(path: &str) -> std::path::PathBuf {
    let plain = codeterm_paths::strip_verbatim_prefix(path);
    std::fs::canonicalize(&plain).unwrap_or_else(|_| std::path::PathBuf::from(plain))
}

/// The lane slug when `worktree` sits directly in the canonical checkout's
/// lane root; `None` for the checkout itself or any other location.
pub fn lane_slug_under_root(canonical: &str, worktree: &str) -> Option<String> {
    let root = lane_worktree_root(&resolved(canonical)).ok()?;
    let lane = resolved(worktree);
    let parent = lane.parent()?;
    (comparable(&parent.to_string_lossy()) == comparable(&root.to_string_lossy()))
        .then(|| {
            lane.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .flatten()
}

/// How `porcelain` (`git worktree list --porcelain`) registers `worktree` as a
/// linked worktree: `None` when it is not one (the first record is the main
/// checkout and never counts), else its checked-out branch (`None` if detached).
pub fn linked_worktree_branch(porcelain: &str, worktree: &str) -> Option<Option<String>> {
    let expected = comparable(&resolved(worktree).to_string_lossy());
    let porcelain = porcelain.replace("\r\n", "\n");
    let mut records = porcelain
        .split("\n\n")
        .filter(|record| !record.trim().is_empty());
    records.next();
    records.find_map(|record| {
        let listed = record
            .lines()
            .find_map(|line| line.strip_prefix("worktree "))?;
        let matches = comparable(listed) == expected
            || comparable(&resolved(listed).to_string_lossy()) == expected;
        matches.then(|| {
            record
                .lines()
                .find_map(|line| line.strip_prefix("branch refs/heads/").map(str::to_string))
        })
    })
}

pub fn registered_worktree_branch(
    canonical: &str,
    worktree: &str,
) -> Result<Option<Option<String>>, String> {
    let listing = git(canonical, &["worktree", "list", "--porcelain"])?;
    Ok(linked_worktree_branch(
        &String::from_utf8_lossy(&listing.stdout),
        worktree,
    ))
}

fn comparable(path: &str) -> String {
    let path = codeterm_paths::strip_verbatim_prefix(path).replace('\\', "/");
    let path = path.trim_end_matches('/');
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase()
    } else {
        path.into()
    }
}
