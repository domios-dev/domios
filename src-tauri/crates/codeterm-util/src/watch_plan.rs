use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

mod path_case;
pub use path_case::PathCase;

pub const MAX_WATCHES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryWatch {
    pub path: PathBuf,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedDirectory {
    pub path: PathBuf,
    pub heavy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPlan {
    pub watches: Vec<DirectoryWatch>,
    pub excluded: Vec<ExcludedDirectory>,
}

pub fn covering_set(root: &Path) -> Result<Vec<DirectoryWatch>, String> {
    covering_set_preserving(root, &[])
}

pub fn covering_set_preserving(
    root: &Path,
    tracked: &[PathBuf],
) -> Result<Vec<DirectoryWatch>, String> {
    directory_plan(root, tracked, &[]).map(|plan| plan.watches)
}

pub fn directory_plan(
    root: &Path,
    tracked: &[PathBuf],
    sources: &[PathBuf],
) -> Result<DirectoryPlan, String> {
    plan(root, tracked, sources, MAX_WATCHES)
}

#[cfg(test)]
fn covering_set_with_limit(root: &Path, limit: usize) -> Result<Vec<DirectoryWatch>, String> {
    plan(root, &[], &[], limit).map(|plan| plan.watches)
}

fn rules(root: &Path, files: &[PathBuf]) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(root);
    for file in files {
        if !file.exists() {
            continue;
        }
        if let Some(error) = builder.add(file) {
            return Err(format!("read {}: {error}", file.display()));
        }
    }
    builder.build().map_err(|error| error.to_string())
}

fn plan(
    root: &Path,
    tracked: &[PathBuf],
    sources: &[PathBuf],
    limit: usize,
) -> Result<DirectoryPlan, String> {
    let tracked: Vec<_> = tracked.iter().map(|path| root.join(path)).collect();
    let mut inherited = vec![rules(root, sources)?];
    let mut excluded = Vec::new();
    let watches = classify(root, &mut inherited, &tracked, false, false, &mut excluded)?.0;
    if watches.len() > limit {
        let error = format!("watch subscription limit exceeded for {}: {} directories, maximum {limit}; repository watching refused", root.display(), watches.len());
        log::warn!("{error}");
        return Err(error);
    }
    Ok(DirectoryPlan { watches, excluded })
}

fn heavy_name(path: &Path) -> bool {
    let case = PathCase::native();
    case.same(path, Path::new("target")) || case.same(path, Path::new("node_modules"))
}

pub fn is_heavy_path(path: &Path) -> bool {
    path.components()
        .any(|component| heavy_name(Path::new(component.as_os_str())))
}

fn classify(
    root: &Path,
    inherited: &mut Vec<Gitignore>,
    tracked: &[PathBuf],
    inside_ignored: bool,
    inside_heavy: bool,
    excluded: &mut Vec<ExcludedDirectory>,
) -> Result<(Vec<DirectoryWatch>, bool), String> {
    inherited.push(rules(root, &[root.join(".gitignore")])?);
    let mut children = Vec::new();
    let mut restricted = inside_ignored;
    let mut entries = std::fs::read_dir(root)
        .map_err(|error| format!("read {}: {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            restricted = true;
            continue;
        }
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        let matched = inherited
            .iter()
            .rev()
            .map(|rules| rules.matched(&path, true))
            .find(|matched| !matched.is_none());
        let heavy = inside_heavy || heavy_name(Path::new(&entry.file_name()));
        let ignored = inside_ignored || heavy || matched.is_some_and(|matched| matched.is_ignore());
        if entry.file_name() == ".git"
            || (ignored
                && !tracked
                    .iter()
                    .any(|file| PathCase::native().within(file, &path)))
        {
            excluded.push(ExcludedDirectory { path, heavy });
            restricted = true;
            continue;
        }
        let (plans, child_restricted) =
            classify(&path, inherited, tracked, ignored, heavy, excluded)?;
        restricted |= child_restricted;
        children.extend(plans);
    }
    inherited.pop();
    let current = DirectoryWatch {
        path: root.to_path_buf(),
        recursive: !restricted,
    };
    if restricted {
        children.insert(0, current);
        Ok((children, true))
    } else {
        Ok((vec![current], false))
    }
}

#[cfg(test)]
mod tests;
