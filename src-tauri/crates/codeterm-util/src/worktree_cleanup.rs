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

fn comparable(path: &str) -> String {
    let path = codeterm_paths::strip_verbatim_prefix(path).replace('\\', "/");
    let path = path.trim_end_matches('/');
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase()
    } else {
        path.into()
    }
}
