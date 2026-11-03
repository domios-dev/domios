use super::*;
use crate::cmd::report_lane::TaskReadinessScope;

pub(super) fn inspect(
    scope: &TaskReadinessScope,
    sha: Option<&str>,
    branch: Option<&str>,
) -> Result<Option<ReportGitMetadata>, ReportRefusal> {
    let (Some(lane), Some(canonical), Some(sha)) = (
        scope.worktree_path.as_deref(),
        scope.canonical_continuation_path.as_deref(),
        sha,
    ) else {
        return Ok(None);
    };
    let lane = ReadinessScope {
        pane_cwd: lane,
        lane_worktree: Some(lane),
        canonical_writer: false,
        recorded_checkout: scope.recorded_checkout,
    }
    .repo()?;
    let lane_tip = git_one(lane, &["rev-parse", "HEAD"]).map_err(git_refusal)?;
    if resolve_commit(lane, sha).as_deref() == Some(lane_tip.as_str()) {
        return Ok(None);
    }
    let canonical = ReadinessScope {
        pane_cwd: canonical,
        lane_worktree: Some(canonical),
        canonical_writer: false,
        recorded_checkout: true,
    }
    .repo()?;
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(git_refusal(
            "canonical continuation requires your literal full commit SHA".into(),
        ));
    }
    for args in [
        vec!["merge-base", "--is-ancestor", &lane_tip, "HEAD"],
        vec!["merge-base", "--is-ancestor", &lane_tip, sha],
        vec!["merge-base", "--is-ancestor", sha, "HEAD"],
    ] {
        git_lines(canonical, &args).map_err(git_refusal)?;
    }
    let parents =
        git_one(canonical, &["rev-list", "--parents", "-n", "1", sha]).map_err(git_refusal)?;
    if parents.split_whitespace().count() != 2 {
        return Err(git_refusal(
            "canonical continuation requires one ordinary task commit".into(),
        ));
    }
    let changed = git_records(
        canonical,
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
    if changed.is_empty() {
        return Err(git_refusal(
            "canonical continuation commit has no changed files".into(),
        ));
    }
    for path in &changed {
        if !scope.files.owns_edit_path(path) {
            return Err(git_refusal(format!(
                "canonical continuation commit contains undeclared or foreign path '{path}'"
            )));
        }
    }
    if let Some(dirty) = load_lane_dirty(lane, None, None)? {
        return Err(readiness_dirty_error(dirty, false));
    }
    let dirty = git_records(
        canonical,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut owned_dirty = Vec::new();
    let mut records = dirty.iter();
    while let Some(record) = records.next() {
        let path = record
            .get(3..)
            .filter(|_| record.as_bytes().get(2) == Some(&b' '))
            .ok_or_else(|| git_refusal("invalid canonical Git status record".into()))?;
        if scope.files.owns_edit_path(path) {
            owned_dirty.push(path.to_string());
        }
        if record.as_bytes()[..2]
            .iter()
            .any(|status| matches!(status, b'R' | b'C'))
        {
            let source = records
                .next()
                .ok_or_else(|| git_refusal("missing canonical rename source".into()))?;
            if scope.files.owns_edit_path(source) {
                owned_dirty.push(source.clone());
            }
        }
    }
    if !owned_dirty.is_empty() {
        return Err(readiness_dirty_error(owned_dirty, true));
    }
    let current_branch = git_one(canonical, &["branch", "--show-current"]).map_err(git_refusal)?;
    if branch.is_some_and(|branch| branch != current_branch) {
        return Err(git_refusal(
            "provided --branch does not match the canonical branch".into(),
        ));
    }
    Ok(Some(ReportGitMetadata {
        branch: Some(current_branch),
        sha: Some(sha.into()),
        changed_files: Some(changed),
    }))
}

fn git_records(cwd: &Path, args: &[&str]) -> Result<Vec<String>, ReportRefusal> {
    let output = git_command(cwd)
        .args(args)
        .output()
        .map_err(|error| git_refusal(error.to_string()))?;
    if !output.status.success() {
        return Err(git_refusal(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let output =
        String::from_utf8(output.stdout).map_err(|error| git_refusal(error.to_string()))?;
    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests;
