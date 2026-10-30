use crate::plan::{Task, TaskExecutionAttempt};
use std::process::Command;

fn git(checkout: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "shared checkout git inspection failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

pub fn shared_checkout_head(checkout: &str) -> Result<String, String> {
    git(checkout, &["rev-parse", "--verify", "HEAD^{commit}"]).map(|head| head.trim().to_string())
}

pub fn validate_shared_report(
    task: &Task,
    attempt: &TaskExecutionAttempt,
    sha: Option<&str>,
) -> Result<Vec<String>, String> {
    let sha = sha
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "shared report requires your literal full commit SHA".to_string())?;
    let checkout = attempt
        .checkout_path
        .as_deref()
        .ok_or("shared attempt has no checkout binding")?;
    let start = attempt
        .shared_start_sha
        .as_deref()
        .ok_or("shared attempt has no start SHA")?;
    if start == sha {
        return Err("shared report must name a commit made after dispatch".into());
    }
    git(checkout, &["merge-base", "--is-ancestor", start, sha])?;
    git(checkout, &["merge-base", "--is-ancestor", sha, "HEAD"])?;
    let parents = git(checkout, &["rev-list", "--parents", "-n", "1", sha])?;
    if parents.split_whitespace().count() != 2 {
        return Err("shared Worker report must name one ordinary task commit".into());
    }
    let files = git(
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
    let files: Vec<String> = files
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Err("shared task commit has no changed files".into());
    }
    for path in &files {
        if !task.files.owns_edit_path(path) {
            return Err(format!(
                "shared commit contains undeclared or foreign path '{path}'"
            ));
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
        if task.files.owns_edit_path(path) {
            return Err(format!("shared task still has uncommitted path '{path}'"));
        }
    }
    Ok(files)
}
