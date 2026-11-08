use crate::plan::{Task, TaskExecutionAttempt, TaskFiles};
use std::process::Command;

/// Why a shared report could not move its task to review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SharedReportError {
    /// The reported commit does not satisfy the shared checkout contract; the Worker can fix it.
    Rejected(String),
    /// Git could not inspect the checkout, so nothing is known about the commit.
    Inspection(String),
}

impl std::fmt::Display for SharedReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) | Self::Inspection(message) => f.write_str(message),
        }
    }
}

fn rejected(message: impl Into<String>) -> SharedReportError {
    SharedReportError::Rejected(message.into())
}

pub(super) fn git(checkout: &str, args: &[&str]) -> Result<String, SharedReportError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| SharedReportError::Inspection(error.to_string()))?;
    if !output.status.success() {
        if output.status.code() == Some(1)
            && args.len() == 4
            && args[..2] == ["merge-base", "--is-ancestor"]
        {
            if args[3] == "HEAD" {
                let head = checkout_head(checkout)?;
                return Err(rejected(format!(
                    "reported commit {} is not on the shared checkout HEAD {head}; it may have been reset or amended",
                    args[2]
                )));
            }
            return Err(rejected(format!(
                "commit {} is not an ancestor of reported commit {}",
                args[2], args[3]
            )));
        }
        return Err(SharedReportError::Inspection(format!(
            "shared checkout git inspection failed (git {}, status {}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| SharedReportError::Inspection(error.to_string()))
}

fn checkout_head(checkout: &str) -> Result<String, SharedReportError> {
    git(checkout, &["rev-parse", "--verify", "HEAD^{commit}"]).map(|head| head.trim().to_string())
}

pub fn shared_checkout_head(checkout: &str) -> Result<String, String> {
    checkout_head(checkout).map_err(|error| error.to_string())
}

pub fn validate_shared_report(
    task: &Task,
    attempt: &TaskExecutionAttempt,
    sha: Option<&str>,
) -> Result<Vec<String>, String> {
    let checkout = attempt
        .checkout_path
        .as_deref()
        .ok_or("shared attempt has no checkout binding")?;
    let start = attempt
        .shared_start_sha
        .as_deref()
        .ok_or("shared attempt has no start SHA")?;
    validate_shared_report_with_git(&task.files, checkout, start, sha, git)
        .map_err(|error| error.to_string())
}

pub(super) fn validate_shared_report_with_git(
    files: &TaskFiles,
    checkout: &str,
    start: &str,
    sha: Option<&str>,
    mut git: impl FnMut(&str, &[&str]) -> Result<String, SharedReportError>,
) -> Result<Vec<String>, SharedReportError> {
    let sha = sha
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| rejected("shared report requires your literal full commit SHA"))?;
    if start == sha {
        return Err(rejected(
            "shared report must name a commit made after dispatch",
        ));
    }
    git(checkout, &["merge-base", "--is-ancestor", start, sha])?;
    git(checkout, &["merge-base", "--is-ancestor", sha, "HEAD"])?;
    let parents = git(checkout, &["rev-list", "--parents", "-n", "1", sha])?;
    if parents.split_whitespace().count() != 2 {
        return Err(rejected(
            "shared Worker report must name one ordinary task commit",
        ));
    }
    let changed = git(
        checkout,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            sha,
        ],
    )?;
    let changed_files: Vec<String> = changed
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    if changed_files.is_empty() {
        return Err(rejected("shared task commit has no changed files"));
    }
    for path in &changed_files {
        if !files.owns_edit_path(path) {
            return Err(rejected(format!(
                "shared commit contains undeclared or foreign path '{path}'"
            )));
        }
    }
    let dirty = git(
        checkout,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    for record in dirty.split('\0').filter(|record| !record.is_empty()) {
        let path = if record.len() > 3 && record.as_bytes().get(2) == Some(&b' ') {
            &record[3..]
        } else {
            record
        };
        if files.owns_edit_path(path) {
            return Err(rejected(format!(
                "shared task still has uncommitted path '{path}'"
            )));
        }
    }
    Ok(changed_files)
}
