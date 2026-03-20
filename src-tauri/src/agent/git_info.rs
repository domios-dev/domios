use git2::Repository;

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub is_dirty: bool,
    pub repo_root: String,
}

pub fn get_git_info(cwd: &str) -> Option<GitInfo> {
    let repo = Repository::discover(cwd).ok()?;
    let head = repo.head().ok()?;
    let branch = head.shorthand().map(|s| s.to_string());
    let is_dirty = repo
        .statuses(None)
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let repo_root = repo.workdir()?.to_string_lossy().to_string();
    Some(GitInfo {
        branch,
        is_dirty,
        repo_root,
    })
}
