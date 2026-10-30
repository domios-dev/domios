use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

pub const MAX_WATCHES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryWatch {
    pub path: PathBuf,
    pub recursive: bool,
}

pub fn covering_set(root: &Path) -> Result<Vec<DirectoryWatch>, String> {
    covering_set_with_limit(root, MAX_WATCHES)
}

fn covering_set_with_limit(root: &Path, limit: usize) -> Result<Vec<DirectoryWatch>, String> {
    let result = classify(root, &mut Vec::new())?.0;
    if result.len() > limit {
        let error = format!("watch subscription limit exceeded for {}: {} directories, maximum {limit}; repository watching refused", root.display(), result.len());
        log::warn!("{error}");
        return Err(error);
    }
    Ok(result)
}

fn classify(
    root: &Path,
    inherited: &mut Vec<Gitignore>,
) -> Result<(Vec<DirectoryWatch>, bool), String> {
    let mut builder = GitignoreBuilder::new(root);
    let ignore_file = root.join(".gitignore");
    if ignore_file.exists() {
        if let Some(error) = builder.add(&ignore_file) {
            return Err(format!("read {}: {error}", ignore_file.display()));
        }
    }
    inherited.push(builder.build().map_err(|error| error.to_string())?);
    let mut children = Vec::new();
    let mut restricted = false;
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
        if entry.file_name() == ".git" || matched.is_some_and(|matched| matched.is_ignore()) {
            restricted = true;
            continue;
        }
        let (plans, child_restricted) = classify(&path, inherited)?;
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
