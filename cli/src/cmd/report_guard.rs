use std::path::Path;
use std::process::Command;

use crate::args::agent::ReportStatusArg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportGitMetadata {
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub changed_files: Option<Vec<String>>,
}

pub fn enrich_ready_report(
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    changed_files: Option<Vec<String>>,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), String> {
    enrich_ready_report_in(Path::new("."), status, branch, sha, changed_files)
}

fn enrich_ready_report_in(
    cwd: &Path,
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    changed_files: Option<Vec<String>>,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), String> {
    if !is_readiness_status(status) {
        return Ok((branch, sha, changed_files));
    }
    let Some(metadata) = inspect_git_worktree(cwd)? else {
        return Ok((branch, sha, changed_files));
    };
    if let (Some(given), Some(head)) = (sha.as_deref(), metadata.sha.as_deref()) {
        if !sha_matches_head(given, head) {
            return Err(format!(
                "readiness report refused: provided --sha {given} does not match HEAD {head}.\n\
                 Commit the lane changes and report the resulting HEAD SHA, or report WIP with `--status partial`."
            ));
        }
    }
    Ok((
        branch.or(metadata.branch),
        sha.or(metadata.sha),
        changed_files.or(metadata.changed_files),
    ))
}

fn is_readiness_status(status: Option<ReportStatusArg>) -> bool {
    matches!(
        status,
        Some(ReportStatusArg::InReview | ReportStatusArg::Done)
    )
}

fn sha_matches_head(given: &str, head: &str) -> bool {
    let given = given.trim();
    let head = head.trim();
    !given.is_empty() && (given == head || head.starts_with(given))
}

fn inspect_git_worktree(cwd: &Path) -> Result<Option<ReportGitMetadata>, String> {
    if !git_success(cwd, &["rev-parse", "--is-inside-work-tree"]) {
        return Ok(None);
    }
    let dirty = git_lines(cwd, &["status", "--porcelain"])?;
    if !dirty.is_empty() {
        return Err(readiness_dirty_error(&dirty));
    }
    let sha = git_one(cwd, &["rev-parse", "HEAD"])?;
    let branch = git_one(cwd, &["branch", "--show-current"]).ok();
    let changed = git_lines(
        cwd,
        &[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            "HEAD",
        ],
    )?;
    Ok(Some(ReportGitMetadata {
        branch,
        sha: Some(sha),
        changed_files: Some(changed),
    }))
}

fn readiness_dirty_error(status_lines: &[String]) -> String {
    let files = status_lines
        .iter()
        .take(8)
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>()
        .join("\n  ");
    format!(
        "readiness report refused: this git worktree has uncommitted changes.\n\
         Commit the lane changes, then rerun `codeterm agent report --status in_review ...`, \
         or report WIP with `--status partial`.\n\
         Dirty files:\n  {files}"
    )
}

fn git_success(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn git_one(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let lines = git_lines(cwd, args)?;
    lines
        .into_iter()
        .next()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("git {} returned no value", args.join(" ")))
}

fn git_lines(cwd: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git {} failed to start: {}", args.join(" "), e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init"]);
        run(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        run(dir.path(), &["config", "user.name", "Test User"]);
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(dir.path(), &["add", "a.txt"]);
        run(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn dirty_readiness_report_is_refused() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let err = inspect_git_worktree(dir.path()).unwrap_err();

        assert!(err.contains("readiness report refused"));
        assert!(err.contains("--status partial"));
        assert!(err.contains("a.txt"));
    }

    #[test]
    fn clean_repo_returns_head_metadata() {
        let dir = repo();

        let metadata = inspect_git_worktree(dir.path()).unwrap().unwrap();

        assert!(metadata.sha.unwrap().len() >= 7);
        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn stale_readiness_sha_is_refused() {
        let err = {
            let dir = repo();
            enrich_ready_report_in(
                dir.path(),
                Some(ReportStatusArg::InReview),
                None,
                Some("deadbeef".into()),
                None,
            )
            .unwrap_err()
        };

        assert!(err.contains("provided --sha deadbeef does not match HEAD"));
        assert!(err.contains("--status partial"));
    }

    #[test]
    fn non_git_directory_is_ignored() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(inspect_git_worktree(dir.path()).unwrap(), None);
    }

    #[test]
    fn partial_report_does_not_inspect_git() {
        let out = enrich_ready_report(
            Some(ReportStatusArg::Partial),
            None,
            None,
            Some(vec!["manual.rs".into()]),
        )
        .unwrap();

        assert_eq!(out.2, Some(vec!["manual.rs".into()]));
    }
}
