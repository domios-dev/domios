use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use codeterm_util::process_util;
use parking_lot::RwLock;

pub fn global_plan_database_path() -> PathBuf {
    codeterm_paths::data_dir().join("plans.db")
}

pub fn project_root(cwd: &Path) -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    if test_project_root(cwd).is_some() {
        return cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    }
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(root) = cache.read().get(cwd) {
        return root.clone();
    }
    let root = if !has_git_marker(cwd) {
        cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
    } else if let Some(root) = main_worktree_root(cwd) {
        root
    } else if let Some(top) = worktree_top(cwd) {
        top
    } else {
        cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
    };
    cache.write().insert(cwd.to_path_buf(), root.clone());
    root
}

/// Cheap filesystem probe that lets non-repository cwds skip the git spawns.
fn has_git_marker(cwd: &Path) -> bool {
    cwd.ancestors().any(|dir| dir.join(".git").exists())
}

/// Resolves many cwds with at most `parallelism` concurrent probes, warming the shared cache.
pub fn warm_project_roots(cwds: &[PathBuf], parallelism: usize) {
    let width = parallelism.max(1);
    for batch in cwds.chunks(width) {
        std::thread::scope(|scope| {
            for cwd in batch {
                scope.spawn(move || {
                    project_root(cwd);
                });
            }
        });
    }
}

fn worktree_top(cwd: &Path) -> Option<PathBuf> {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let out = cmd
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if top.is_empty() {
        return None;
    }
    let top = PathBuf::from(top);
    Some(top.canonicalize().unwrap_or(top))
}

#[cfg(any(test, feature = "test-support"))]
const TEST_PROJECT_ROOT_MARKER: &str = ".codeterm-test-project-root";

#[cfg(any(test, feature = "test-support"))]
pub fn mark_test_project_root(root: &Path) {
    if root.is_dir() {
        let _ = std::fs::write(root.join(TEST_PROJECT_ROOT_MARKER), "");
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_project_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    loop {
        if current.join(TEST_PROJECT_ROOT_MARKER).is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn main_worktree_root(cwd: &Path) -> Option<PathBuf> {
    let (_, common_dir) = codeterm_util::git_checkout::paths(cwd).ok()?;
    if common_dir.file_name().and_then(|name| name.to_str()) != Some(".git") {
        return None;
    }
    let root = common_dir.parent()?.to_path_buf();
    Some(root.canonicalize().unwrap_or(root))
}

pub fn plan_state_root() -> PathBuf {
    global_plan_database_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("plans")
}

fn plan_state_root_for(_project: &Path) -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(root) = test_project_root(_project) {
        return root.join(".domios").join("plans");
    }
    plan_state_root()
}

pub fn plan_bucket_dir(
    project: &Path,
    status: &crate::plan::PlanStatus,
) -> std::io::Result<PathBuf> {
    let dir = plan_state_root_for(project).join(plan_bucket(status));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plan_state_dir(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> std::io::Result<PathBuf> {
    let dir = plan_state_dir_unchecked(project, status, plan_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plan_dir(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> std::io::Result<PathBuf> {
    plan_state_dir(project, status, plan_id)
}

pub fn plan_state_dir_unchecked(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> PathBuf {
    plan_state_root_for(project)
        .join(plan_bucket(status))
        .join(plan_id)
}

pub fn plan_json_path(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> std::io::Result<PathBuf> {
    Ok(plan_dir(project, status, plan_id)?.join("plan.json"))
}

pub fn plan_sidecar_root(plan: &crate::plan::Plan) -> &Path {
    if plan.canonical_project_path.trim().is_empty() {
        Path::new(&plan.project_path)
    } else {
        Path::new(&plan.canonical_project_path)
    }
}

pub fn spec_sidecar_root(spec: &crate::plan::Spec) -> &Path {
    if spec.canonical_project_path.trim().is_empty() {
        Path::new(&spec.project_path)
    } else {
        Path::new(&spec.canonical_project_path)
    }
}

pub fn normalize_plan_paths(plan: &mut crate::plan::Plan, root_path: &Path) {
    let root = root_path.display().to_string();
    plan.project_path = root.clone();
    if plan.canonical_project_path.trim().is_empty() {
        plan.canonical_project_path = root;
    } else {
        plan.canonical_project_path = normalize_canonical_home(&plan.canonical_project_path, &root);
    }
}

pub fn normalize_spec_paths(spec: &mut crate::plan::Spec, root_path: &Path) {
    let root = root_path.display().to_string();
    spec.project_path = root.clone();
    if spec.canonical_project_path.trim().is_empty() {
        spec.canonical_project_path = root;
    } else {
        spec.canonical_project_path = normalize_canonical_home(&spec.canonical_project_path, &root);
    }
}

fn normalize_canonical_home(raw: &str, fallback: &str) -> String {
    let path = Path::new(raw);
    if path.is_dir() {
        project_root(path).display().to_string()
    } else {
        fallback.to_string()
    }
}

pub fn plan_bucket(status: &crate::plan::PlanStatus) -> &'static str {
    match status {
        crate::plan::PlanStatus::Archived => "archive",
        _ => "active",
    }
}

pub fn specs_dir(project: &Path) -> std::io::Result<PathBuf> {
    let dir = project.join(".domios").join("specs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests;
