use std::path::{Path, PathBuf};
use std::process::Command;

pub fn project_root(cwd: &Path) -> PathBuf {
    #[cfg(test)]
    if test_project_root_marker(cwd).is_some() {
        return cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    }
    if let Some(root) = main_worktree_root(cwd) {
        return root;
    }
    if let Ok(out) = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if out.status.success() {
            let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !top.is_empty() {
                let top = PathBuf::from(top);
                return top.canonicalize().unwrap_or(top);
            }
        }
    }
    cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
}

#[cfg(test)]
const TEST_PROJECT_ROOT_MARKER: &str = ".codeterm-test-project-root";

#[cfg(test)]
pub fn mark_test_project_root(root: &Path) {
    if root.is_dir() {
        let _ = std::fs::write(root.join(TEST_PROJECT_ROOT_MARKER), "");
    }
}

#[cfg(test)]
fn test_project_root_marker(cwd: &Path) -> Option<PathBuf> {
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

pub fn plans_dir(project: &Path) -> std::io::Result<PathBuf> {
    let dir = project.join(".codeterm").join("plans");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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
        let out = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
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
}
