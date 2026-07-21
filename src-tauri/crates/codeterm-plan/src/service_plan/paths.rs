use std::path::{Path, PathBuf};
use std::process::Command;

use codeterm_util::process_util;

pub fn global_plan_database_path() -> PathBuf {
    codeterm_paths::data_dir().join("plans.db")
}

pub fn project_root(cwd: &Path) -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    if test_project_root(cwd).is_some() {
        return cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    }
    if let Some(root) = main_worktree_root(cwd) {
        return root;
    }
    if let Some(top) = worktree_top(cwd) {
        return top;
    }
    cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
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
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let out = cmd
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

pub fn plans_dir(project: &Path) -> std::io::Result<PathBuf> {
    let dir = project.join(".codeterm").join("plans");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plan_bucket_dir(
    project: &Path,
    status: &crate::plan::PlanStatus,
) -> std::io::Result<PathBuf> {
    let dir = plans_dir(project)?.join(plan_bucket(status));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plan_dir(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> std::io::Result<PathBuf> {
    let stem = if plan_id == crate::plan::INBOX_PLAN_ID {
        crate::plan::INBOX_PLAN_FILE_STEM
    } else {
        plan_id
    };
    let dir = plan_bucket_dir(project, status)?.join(stem);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plan_json_path(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> std::io::Result<PathBuf> {
    Ok(plan_dir(project, status, plan_id)?.join("plan.json"))
}

pub fn plan_json_path_unchecked(
    project: &Path,
    status: &crate::plan::PlanStatus,
    plan_id: &str,
) -> PathBuf {
    let stem = if plan_id == crate::plan::INBOX_PLAN_ID {
        crate::plan::INBOX_PLAN_FILE_STEM
    } else {
        plan_id
    };
    project
        .join(".codeterm")
        .join("plans")
        .join(plan_bucket(status))
        .join(stem)
        .join("plan.json")
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
    let dir = project.join(".codeterm").join("specs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(cwd: &Path, args: &[&str]) {
        let mut cmd = Command::new("git");
        process_util::no_window_std(&mut cmd);
        let out = cmd.current_dir(cwd).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn project_root_resolves_linked_worktree_to_main_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let linked = tmp.path().join("linked");
        std::fs::create_dir(&main).unwrap();
        run_git(&main, &["init"]);
        run_git(&main, &["config", "user.email", "test@example.com"]);
        run_git(&main, &["config", "user.name", "Test User"]);
        std::fs::write(main.join("README.md"), "root\n").unwrap();
        run_git(&main, &["add", "README.md"]);
        run_git(&main, &["commit", "-m", "init"]);
        run_git(
            &main,
            &[
                "worktree",
                "add",
                linked.to_str().unwrap(),
                "-b",
                "linked-test",
            ],
        );

        let main_root = project_root(&main);
        let linked_root = project_root(&linked);

        assert_eq!(main_root, main.canonicalize().unwrap());
        assert_eq!(linked_root, main_root);
    }

    #[test]
    fn project_root_ignores_linked_worktree_codeterm_for_plan_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let linked = tmp.path().join("linked");
        std::fs::create_dir(&main).unwrap();
        run_git(&main, &["init"]);
        run_git(&main, &["config", "user.email", "test@example.com"]);
        run_git(&main, &["config", "user.name", "Test User"]);
        std::fs::write(main.join("README.md"), "root\n").unwrap();
        run_git(&main, &["add", "README.md"]);
        run_git(&main, &["commit", "-m", "init"]);
        run_git(
            &main,
            &[
                "worktree",
                "add",
                linked.to_str().unwrap(),
                "-b",
                "linked-test",
            ],
        );

        std::fs::create_dir_all(linked.join(".codeterm").join("plans")).unwrap();

        assert_eq!(project_root(&linked), main.canonicalize().unwrap());
    }
}
