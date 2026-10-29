use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::util::ChannelSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GitVersion(u32, u32, u32);

impl GitVersion {
    fn parse(output: &str) -> Option<Self> {
        let version = output.trim().strip_prefix("git version ")?;
        let mut parts = version.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.split(|c: char| !c.is_ascii_digit()).next()?;
        Some(Self(major, minor, patch.parse().ok()?))
    }
}

struct GitTool {
    path: PathBuf,
    version: Option<GitVersion>,
}

fn newest_git(
    candidates: impl IntoIterator<Item = PathBuf>,
    mut probe: impl FnMut(&Path) -> Option<GitVersion>,
) -> GitTool {
    let mut best: Option<(PathBuf, GitVersion)> = None;
    for path in candidates {
        if let Some(version) = probe(&path) {
            if best
                .as_ref()
                .is_none_or(|(_, previous)| version > *previous)
            {
                best = Some((path, version));
            }
        }
    }
    match best {
        Some((path, version)) => GitTool {
            path,
            version: Some(version),
        },
        None => GitTool {
            path: PathBuf::from("git"),
            version: None,
        },
    }
}

#[cfg(test)]
fn probe_git(path: &Path) -> Option<GitVersion> {
    probe_git_before(
        path,
        Instant::now() + Duration::from_secs(1),
        crate::git_probe::epoch(),
    )
}

fn probe_git_before(path: &Path, deadline: Instant, epoch: u64) -> Option<GitVersion> {
    let output = crate::git_probe::capture(path, deadline, epoch)?;
    GitVersion::parse(&String::from_utf8_lossy(&output))
}

fn selected_git() -> Result<&'static GitTool, String> {
    static GIT: OnceLock<Result<GitTool, String>> = OnceLock::new();
    GIT.get_or_init(|| {
        let executable = if cfg!(windows) { "git.exe" } else { "git" };
        let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(executable))
                    .collect()
            })
            .unwrap_or_default();
        if cfg!(target_os = "macos") {
            candidates.extend(
                [
                    "/usr/local/bin/git",
                    "/opt/homebrew/bin/git",
                    "/Library/Developer/CommandLineTools/usr/bin/git",
                    "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
                    "/usr/bin/git",
                ]
                .map(PathBuf::from),
            );
        }
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|path| seen.insert(path.clone()));
        let deadline = Instant::now() + Duration::from_secs(5);
        let epoch = crate::git_probe::epoch();
        let selected = newest_git(candidates, |path| {
            probe_git_before(
                path,
                deadline.min(Instant::now() + Duration::from_secs(1)),
                epoch,
            )
        });
        if crate::git_probe::epoch() != epoch {
            Err("plugin channel git operation cancelled".into())
        } else {
            Ok(selected)
        }
    })
    .as_ref()
    .map_err(Clone::clone)
}

pub(crate) fn program() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("CODETERM_TEST_GIT_BIN") {
        return Ok(PathBuf::from(path));
    }
    Ok(selected_git()?.path.clone())
}

pub(crate) fn supports_sparse() -> Result<bool, String> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("CODETERM_TEST_GIT_BIN") {
        return Ok(probe_git(Path::new(&path)).is_some_and(|v| v >= GitVersion(2, 25, 0)));
    }
    Ok(selected_git()?
        .version
        .is_some_and(|v| v >= GitVersion(2, 25, 0)))
}

pub(crate) fn clone_commands(
    source: &ChannelSource,
    tmp: &Path,
    sparse_supported: bool,
) -> Vec<Vec<String>> {
    let mut clone = vec![
        "clone".into(),
        "-c".into(),
        "core.longpaths=true".into(),
        "--depth".into(),
        "1".into(),
    ];
    if sparse_supported {
        clone.push("--filter=blob:none".into());
        if source.subdir.is_some() {
            clone.push("--sparse".into());
        }
    }
    clone.extend([source.base.clone(), tmp.to_string_lossy().into_owned()]);
    let mut commands = vec![clone];
    if let Some(git_ref) = &source.git_ref {
        commands.push(vec![
            "fetch".into(),
            "--depth".into(),
            "1".into(),
            "origin".into(),
            git_ref.clone(),
        ]);
        commands.push(vec!["checkout".into(), "FETCH_HEAD".into()]);
    }
    if sparse_supported {
        if let Some(subdir) = &source.subdir {
            commands.push(vec![
                "sparse-checkout".into(),
                "set".into(),
                "--cone".into(),
                subdir.clone(),
            ]);
        }
    }
    commands
}

#[cfg(test)]
mod tests;
