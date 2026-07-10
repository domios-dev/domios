use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::args::agent::ReportStatusArg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportGitMetadata {
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub changed_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRefusal {
    pub reason: String,
    pub dirty_files: Vec<String>,
}

impl ReportRefusal {
    pub fn message(&self) -> String {
        let mut lines = vec![
            self.reason.clone(),
            "Next commands:".to_string(),
            "  git status --short".to_string(),
            "  git add <files>".to_string(),
            "  git commit -m \"<message>\"".to_string(),
            "  codeterm agent report --status in_review --sha $(git rev-parse HEAD) \"...\""
                .to_string(),
            "Or report WIP with:".to_string(),
            "  codeterm agent report --status partial \"...\"".to_string(),
        ];
        if !self.dirty_files.is_empty() {
            lines.push("Dirty lane files:".to_string());
            lines.extend(self.dirty_files.iter().map(|file| format!("  {file}")));
        }
        lines.join("\n")
    }
}

#[allow(clippy::type_complexity)]
pub fn enrich_ready_report(
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    enrich_ready_report_in(
        Path::new("."),
        status,
        branch,
        sha,
        base,
        changed_files,
        acknowledge_dirty,
    )
}

#[allow(clippy::type_complexity)]
fn enrich_ready_report_in(
    cwd: &Path,
    status: Option<ReportStatusArg>,
    branch: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    changed_files: Option<Vec<String>>,
    acknowledge_dirty: bool,
) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
    if !is_readiness_status(status) {
        return Ok((branch, sha, changed_files));
    }
    let Some(metadata) = inspect_git_worktree(cwd, base.as_deref())? else {
        return Ok((branch, sha, changed_files));
    };
    if let (Some(given), Some(head)) = (sha.as_deref(), metadata.sha.as_deref()) {
        if !sha_matches_head(given, head) {
            return Err(ReportRefusal {
                reason: format!(
                    "readiness report refused: provided --sha {given} does not match HEAD {head}."
                ),
                dirty_files: Vec::new(),
            });
        }
    }
    if let (Some(ref given), Some(ref head)) = (branch.as_deref(), metadata.branch.as_deref()) {
        if given != head {
            return Err(ReportRefusal {
                reason: format!(
                    "readiness report refused: provided --branch '{given}' does not match current branch '{head}'."
                ),
                dirty_files: Vec::new(),
            });
        }
    }
    if !acknowledge_dirty {
        if let Some(dirty) = load_lane_dirty(cwd) {
            return Err(readiness_dirty_error(dirty));
        }
    }
    Ok((
        branch.or(metadata.branch),
        sha.or(metadata.sha),
        changed_files.or(metadata.changed_files),
    ))
}

fn load_lane_dirty(cwd: &Path) -> Option<Vec<String>> {
    let dirty = git_lines(cwd, &["status", "--porcelain", "-uall"]).ok()?;
    let lane_dirty = lane_dirty_files(&dirty);
    if lane_dirty.is_empty() {
        None
    } else {
        Some(lane_dirty)
    }
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

fn inspect_git_worktree(
    cwd: &Path,
    base: Option<&str>,
) -> Result<Option<ReportGitMetadata>, ReportRefusal> {
    if !git_success(cwd, &["rev-parse", "--is-inside-work-tree"]) {
        return Ok(None);
    }
    let sha = git_one(cwd, &["rev-parse", "HEAD"]).map_err(git_refusal)?;
    let branch = git_one(cwd, &["branch", "--show-current"]).ok();
    let changed = load_changed_files(cwd, base).map_err(git_refusal)?;
    Ok(Some(ReportGitMetadata {
        branch,
        sha: Some(sha),
        changed_files: Some(changed),
    }))
}

fn load_changed_files(cwd: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let committed = match resolve_base_commit(cwd, base)? {
        Some(base) => git_lines(cwd, &["diff", "--name-only", &base, "HEAD"])?,
        None => Vec::new(),
    };
    let overlay = lane_dirty_files(&git_lines(cwd, &["status", "--porcelain", "-uall"])?);
    let changed: BTreeSet<String> = committed.into_iter().chain(overlay).collect();
    Ok(changed.into_iter().collect())
}

fn resolve_base_commit(cwd: &Path, base: Option<&str>) -> Result<Option<String>, String> {
    let Some(base) = base.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if base.starts_with('-') {
        return Err("invalid report base: refs starting with '-' are rejected".into());
    }
    git_one(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map(Some)
    .map_err(|_| format!("invalid report base: '{base}' is not a commit in this worktree"))
}

fn git_refusal(reason: String) -> ReportRefusal {
    ReportRefusal {
        reason: format!("readiness report refused: {reason}"),
        dirty_files: Vec::new(),
    }
}

fn readiness_dirty_error(dirty_files: Vec<String>) -> ReportRefusal {
    ReportRefusal {
        reason: "readiness report refused: this git worktree has uncommitted lane changes."
            .to_string(),
        dirty_files: dirty_files.into_iter().take(8).collect(),
    }
}

fn lane_dirty_files(status_lines: &[String]) -> Vec<String> {
    status_lines
        .iter()
        .flat_map(|line| porcelain_paths(line))
        .filter(|path| !is_tool_owned_path(path))
        .collect()
}

fn porcelain_paths(line: &str) -> Vec<String> {
    let Some(rest) = line.get(3..) else {
        return Vec::new();
    };
    rest.split(" -> ")
        .map(unquote_git_path)
        .filter(|path| !path.is_empty())
        .collect()
}

fn unquote_git_path(path: &str) -> String {
    path.trim().trim_matches('"').to_string()
}

fn is_tool_owned_path(path: &str) -> bool {
    path == ".codeterm/plans" || path.starts_with(".codeterm/plans/")
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
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
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
        run(dir.path(), &["init", "--initial-branch", "main"]);
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

    fn repo_with_linked_worktree() -> (TempDir, PathBuf, PathBuf) {
        let dir = repo();
        let main = dir.path().to_path_buf();
        let linked = dir.path().join("linked");
        let linked_str = linked.to_string_lossy().into_owned();
        run(
            dir.path(),
            &["worktree", "add", "-b", "lane/report", &linked_str],
        );
        (dir, main, linked)
    }

    fn branch_head(dir: &Path) -> String {
        git_one(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    #[test]
    fn dirty_readiness_report_is_refused() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert!(err.message().contains("readiness report refused"));
        assert!(err.message().contains("--status partial"));
        assert_eq!(err.dirty_files, vec!["a.txt"]);
    }

    #[test]
    fn acknowledge_dirty_skips_dirty_check() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let (branch, sha, changed) = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

        assert!(sha.unwrap().len() >= 7);
        assert!(changed.unwrap().contains(&"a.txt".to_string()));
        assert_eq!(branch.as_deref(), Some("main"));
    }

    #[test]
    fn tool_owned_plan_mirror_dirt_is_ignored() {
        let dir = repo();
        let plans_dir = dir.path().join(".codeterm/plans");
        fs::create_dir_all(&plans_dir).unwrap();
        fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();

        let (_branch, sha, _changed) = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();

        assert!(sha.unwrap().len() >= 7);
    }

    #[test]
    fn mixed_tool_and_lane_dirt_refuses_only_lane_files() {
        let dir = repo();
        let plans_dir = dir.path().join(".codeterm/plans");
        fs::create_dir_all(&plans_dir).unwrap();
        fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();
        fs::write(dir.path().join("src.rs"), "lane dirt\n").unwrap();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert_eq!(err.dirty_files, vec!["src.rs"]);
        assert!(!err.message().contains(".codeterm/plans"));
    }

    #[test]
    fn clean_repo_returns_head_metadata() {
        let dir = repo();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert!(metadata.sha.unwrap().len() >= 7);
        assert!(metadata.changed_files.unwrap().is_empty());
    }

    #[test]
    fn modified_file_reports_exactly_one_changed_path() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn untracked_file_reports_exactly_one_changed_path() {
        let dir = repo();
        fs::write(dir.path().join("new.txt"), "new\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["new.txt"]);
    }

    #[test]
    fn linked_worktree_stays_clean_when_main_checkout_is_dirty() {
        let (_dir, main, linked) = repo_with_linked_worktree();
        fs::write(main.join("a.txt"), "dirty main\n").unwrap();

        let metadata = inspect_git_worktree(&linked, Some("HEAD"))
            .unwrap()
            .unwrap();

        assert!(metadata.changed_files.unwrap().is_empty());
    }

    #[test]
    fn one_committed_lane_commit_reports_its_files_since_base() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn multiple_commits_report_union_since_base() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "first"]);
        fs::write(dir.path().join("b.txt"), "bee\n").unwrap();
        run(dir.path(), &["add", "b.txt"]);
        run(dir.path(), &["commit", "-m", "second"]);

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn dirty_overlay_dedupes_with_committed_diff() {
        let dir = repo();
        let base = branch_head(dir.path());
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);
        fs::write(dir.path().join("a.txt"), "three\n").unwrap();

        let metadata = inspect_git_worktree(dir.path(), Some(&base))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    }

    #[test]
    fn invalid_base_is_refused() {
        let dir = repo();
        let err = inspect_git_worktree(dir.path(), Some("not-a-real-base"))
            .unwrap_err()
            .message();
        assert!(err.contains("invalid report base"));
    }

    #[test]
    fn leading_dash_base_is_refused() {
        let dir = repo();
        let err = inspect_git_worktree(dir.path(), Some("--bad"))
            .unwrap_err()
            .message();
        assert!(err.contains("starting with '-'"));
    }

    #[test]
    fn slash_ref_base_resolves_when_it_is_a_commit_ref() {
        let dir = repo();
        run(dir.path(), &["checkout", "-b", "lane/valid"]);
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        run(dir.path(), &["commit", "-am", "lane change"]);
        let lane_head = branch_head(dir.path());
        run(dir.path(), &["checkout", "main"]);

        let metadata = inspect_git_worktree(dir.path(), Some("refs/heads/lane/valid"))
            .unwrap()
            .unwrap();

        assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
        assert_ne!(metadata.sha.unwrap(), lane_head);
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
                None,
                false,
            )
            .unwrap_err()
        };

        assert!(err
            .message()
            .contains("provided --sha deadbeef does not match HEAD"));
        assert!(err.message().contains("--status partial"));
    }

    #[test]
    fn wrong_branch_is_refused() {
        let dir = repo();

        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            Some("lane/wrong".into()),
            None,
            None,
            None,
            false,
        )
        .unwrap_err();

        assert!(err.message().contains("--branch"));
        assert!(err.message().contains("does not match current branch"));
    }

    #[test]
    fn non_git_directory_is_ignored() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(inspect_git_worktree(dir.path(), None).unwrap(), None);
    }

    #[test]
    fn partial_report_does_not_inspect_git() {
        let out = enrich_ready_report(
            Some(ReportStatusArg::Partial),
            None,
            None,
            None,
            Some(vec!["manual.rs".into()]),
            false,
        )
        .unwrap();

        assert_eq!(out.2, Some(vec!["manual.rs".into()]));
    }
}
