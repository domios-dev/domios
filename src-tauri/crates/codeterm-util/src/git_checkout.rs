use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum GitCheckoutError {
    Command(String),
    UnexpectedOutput {
        git_version: String,
        reason: String,
    },
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for GitCheckoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(reason) => write!(f, "git checkout inspection failed: {reason}"),
            Self::UnexpectedOutput {
                git_version,
                reason,
            } => {
                write!(
                    f,
                    "unexpected rev-parse output from {git_version}: {reason}"
                )
            }
            Self::Canonicalize { path, source } => {
                write!(
                    f,
                    "cannot canonicalize Git path '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for GitCheckoutError {}

pub fn command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    command
        .current_dir(cwd)
        .args(["rev-parse", "--git-dir", "--git-common-dir"]);
    command
}

pub fn paths(cwd: &Path) -> Result<(PathBuf, PathBuf), GitCheckoutError> {
    let output = command(cwd)
        .output()
        .map_err(|error| GitCheckoutError::Command(error.to_string()))?;
    if !output.status.success() {
        return Err(GitCheckoutError::Command(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    parse_output(cwd, &output.stdout)
}

pub fn parse_output(cwd: &Path, stdout: &[u8]) -> Result<(PathBuf, PathBuf), GitCheckoutError> {
    let raw = parse_paths(cwd, stdout, || {
        let mut command = Command::new("git");
        crate::process_util::no_window_std(&mut command);
        command
            .current_dir(cwd)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_else(|| "git (version unavailable)".into())
    })?;
    let canonicalize = |path: PathBuf| {
        std::fs::canonicalize(&path)
            .map_err(|source| GitCheckoutError::Canonicalize { path, source })
    };
    Ok((canonicalize(raw.0)?, canonicalize(raw.1)?))
}

fn parse_paths(
    cwd: &Path,
    stdout: &[u8],
    git_version: impl FnOnce() -> String,
) -> Result<(PathBuf, PathBuf), GitCheckoutError> {
    parse_path_lines(cwd, stdout).map_err(|reason| GitCheckoutError::UnexpectedOutput {
        git_version: git_version(),
        reason,
    })
}

fn parse_path_lines(cwd: &Path, stdout: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let text = std::str::from_utf8(stdout).map_err(|error| error.to_string())?;
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != 2
        || lines
            .iter()
            .any(|line| line.is_empty() || line.starts_with("--"))
    {
        return Err(format!(
            "expected exactly two nonempty path lines, received {}",
            lines.len()
        ));
    }
    Ok((cwd.join(lines[0]), cwd.join(lines[1])))
}

#[cfg(test)]
mod tests;
