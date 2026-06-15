use git2::Repository;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub is_dirty: bool,
    pub repo_root: String,
    pub added: u64,
    pub deleted: u64,
}

pub fn get_git_info(cwd: &str) -> Option<GitInfo> {
    let repo = Repository::discover(cwd).ok()?;
    let head = repo.head().ok()?;
    let branch = head
        .is_branch()
        .then(|| head.shorthand().map(|s| s.to_string()))
        .flatten();
    let is_dirty = repo.statuses(None).map(|s| !s.is_empty()).unwrap_or(false);
    let repo_root = repo.workdir()?.to_string_lossy().to_string();
    let (added, deleted) = diff_stats(&repo_root);
    Some(GitInfo {
        branch,
        is_dirty,
        repo_root,
        added,
        deleted,
    })
}

fn diff_stats(repo_root: &str) -> (u64, u64) {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_root)
        .arg("--no-pager")
        .args(["diff", "--numstat", "HEAD"]);
    crate::process_util::no_window_std(&mut cmd);

    let Ok(output) = cmd.output() else {
        return (0, 0);
    };
    if !output.status.success() {
        return (0, 0);
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let added = parts.next()?.parse::<u64>().ok()?;
            let deleted = parts.next()?.parse::<u64>().ok()?;
            Some((added, deleted))
        })
        .fold((0, 0), |(ta, td), (a, d)| (ta + a, td + d))
}
