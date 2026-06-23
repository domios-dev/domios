use std::path::{Path, PathBuf};
use std::process::Command;

pub fn project_root(cwd: &Path) -> PathBuf {
    if let Ok(out) = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if out.status.success() {
            let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !top.is_empty() {
                return PathBuf::from(top);
            }
        }
    }
    cwd.to_path_buf()
}

pub fn plans_dir(project: &Path) -> std::io::Result<PathBuf> {
    let dir = project.join(".codeterm").join("plans");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
