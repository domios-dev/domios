use crate::WatchSpec;
use notify::{RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) fn expand(specs: &[WatchSpec]) -> Result<BTreeMap<PathBuf, bool>, String> {
    let mut desired = BTreeMap::new();
    for spec in specs {
        if spec.optional && !spec.path.exists() {
            continue;
        }
        if spec.partitioned {
            for watch in codeterm_util::watch_plan::covering_set(&spec.path)? {
                desired.insert(watch.path, watch.recursive);
            }
        } else {
            desired.insert(spec.path.clone(), spec.recursive);
        }
    }
    if desired.len() > codeterm_util::watch_plan::MAX_WATCHES {
        let error = format!("repository watch subscription limit exceeded: {} directories, maximum {}; watching refused", desired.len(), codeterm_util::watch_plan::MAX_WATCHES);
        log::warn!("{error}");
        return Err(error);
    }
    Ok(desired)
}

pub(crate) fn topology_changed(
    specs: &[WatchSpec],
    active: &BTreeMap<PathBuf, bool>,
    paths: &[PathBuf],
) -> bool {
    paths.iter().any(|path| {
        path.is_dir()
            || path.file_name().is_some_and(|name| name == ".gitignore")
            || path
                .parent()
                .is_some_and(|parent| active.get(parent) == Some(&false))
            || specs.iter().any(|spec| path == &spec.path)
    })
}

pub(crate) fn reconcile(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut BTreeMap<PathBuf, bool>,
    specs: &[WatchSpec],
) -> Result<(), String> {
    let desired = match expand(specs) {
        Ok(desired) => desired,
        Err(error) => {
            for path in active.keys() {
                let _ = watcher.unwatch(path);
            }
            active.clear();
            return Err(error);
        }
    };
    let removed: Vec<_> = active
        .iter()
        .filter(|(path, recursive)| desired.get(*path) != Some(*recursive))
        .map(|(path, _)| path.clone())
        .collect();
    for path in removed {
        remove(watcher, &path)?;
        active.remove(&path);
    }
    for (path, recursive) in desired {
        if active.get(&path) == Some(&recursive) {
            continue;
        }
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher
            .watch(&path, mode)
            .map_err(|error| format!("watch {}: {error}", path.display()))?;
        active.insert(path, recursive);
    }
    Ok(())
}

pub(crate) fn invalidate(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut BTreeMap<PathBuf, bool>,
    changed: &[PathBuf],
) -> Result<(), String> {
    let invalidated: Vec<_> = active
        .keys()
        .filter(|root| changed.iter().any(|path| root.starts_with(path)))
        .cloned()
        .collect();
    for path in invalidated {
        remove(watcher, &path)?;
        active.remove(&path);
    }
    Ok(())
}

fn remove(watcher: &mut (impl Watcher + ?Sized), path: &std::path::Path) -> Result<(), String> {
    match watcher.unwatch(path) {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.kind, notify::ErrorKind::WatchNotFound) || !path.exists() => {
            Ok(())
        }
        Err(error) => Err(format!("unwatch {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests;
