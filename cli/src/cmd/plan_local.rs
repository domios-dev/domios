use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn project_root(project: Option<String>) -> Result<PathBuf, String> {
    let raw = project.unwrap_or_else(|| ".".into());
    let cwd = PathBuf::from(raw)
        .canonicalize()
        .map_err(|e| format!("resolve project path: {e}"))?;
    Ok(main_worktree_root(&cwd).unwrap_or(cwd))
}

fn main_worktree_root(cwd: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let common_dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if common_dir.is_empty() {
        return None;
    }
    let common_dir = PathBuf::from(common_dir);
    if common_dir.file_name().and_then(|name| name.to_str()) != Some(".git") {
        return None;
    }
    let root = common_dir.parent()?.to_path_buf();
    Some(root.canonicalize().unwrap_or(root))
}

pub(crate) fn plan_id_from(plan: &serde_json::Value, path: &Path) -> Option<String> {
    plan.get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            path.parent()?
                .file_name()
                .and_then(|s| s.to_str())
                .map(|stem| {
                    if stem == "_inbox" {
                        "inbox".to_string()
                    } else {
                        stem.to_string()
                    }
                })
        })
}
