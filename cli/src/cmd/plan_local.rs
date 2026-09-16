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

/// Load a plan.yaml as a json Value with its per-file tasks assembled back in,
/// so local tools (doctor, prune) see the same shape the daemon serves.
#[cfg(test)]
pub(crate) fn load_plan_value(path: &Path) -> Result<serde_json::Value, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut plan: serde_json::Value =
        serde_yaml::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tasks = Vec::new();
    if let Some(dir) = path.parent() {
        // goal.yaml is a per-entity file like tasks; fold it back in.
        let goal_path = dir.join("goal.yaml");
        if goal_path.is_file() {
            let raw = std::fs::read_to_string(&goal_path)
                .map_err(|e| format!("{}: {e}", goal_path.display()))?;
            let goal: serde_json::Value =
                serde_yaml::from_str(&raw).map_err(|e| format!("{}: {e}", goal_path.display()))?;
            if let Some(obj) = plan.as_object_mut() {
                obj.insert("goal".into(), goal);
            }
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("tasks")) {
            let mut task_paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
                .collect();
            task_paths.sort();
            for p in task_paths {
                let raw =
                    std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let task: serde_json::Value =
                    serde_yaml::from_str(&raw).map_err(|e| format!("{}: {e}", p.display()))?;
                tasks.push(task);
            }
        }
    }
    if let Some(obj) = plan.as_object_mut() {
        obj.insert("tasks".into(), serde_json::Value::Array(tasks));
    }
    Ok(plan)
}

#[cfg(test)]
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
