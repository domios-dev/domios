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

pub(crate) fn plan_files(plans_dir: &Path, plan_id: Option<&str>) -> Result<Vec<PathBuf>, String> {
    if let Some(id) = plan_id {
        return Ok(["active", "archive"]
            .into_iter()
            .map(|bucket| plans_dir.join(bucket).join(plan_stem(id)).join("plan.json"))
            .filter(|path| path.is_file())
            .collect());
    }
    let mut files = Vec::new();
    for bucket in ["active", "archive"] {
        let bucket_dir = plans_dir.join(bucket);
        let Ok(entries) = std::fs::read_dir(&bucket_dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path().join("plan.json");
            if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn plan_stem(id: &str) -> &str {
    if id == "inbox" {
        "_inbox"
    } else {
        id
    }
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

pub(crate) fn is_inbox_plan(plan: &serde_json::Value, path: &Path) -> bool {
    plan.get("id")
        .and_then(|v| v.as_str())
        .is_some_and(|id| id == "inbox")
        || path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem == "_inbox")
}
