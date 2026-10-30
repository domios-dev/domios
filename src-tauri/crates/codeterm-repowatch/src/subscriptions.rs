use crate::WatchSpec;
use codeterm_util::watch_plan::PathCase;
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
            let repo = git2::Repository::open(&spec.path).map_err(|error| error.to_string())?;
            let tracked = crate::tracked::paths(&repo)?;
            for watch in codeterm_util::watch_plan::covering_set_preserving(&spec.path, &tracked)? {
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
    topology_changed_with_case(specs, active, paths, PathCase::native())
}

pub(crate) fn topology_changed_with_case(
    specs: &[WatchSpec],
    active: &BTreeMap<PathBuf, bool>,
    paths: &[PathBuf],
    case: PathCase,
) -> bool {
    paths.iter().any(|path| {
        path.is_dir()
            || path.file_name().is_some_and(|name| name == ".gitignore")
            || active
                .iter()
                .any(|(parent, recursive)| !recursive && case.direct_child(path, parent))
            || specs.iter().any(|spec| case.same(path, &spec.path))
    })
}

pub(crate) fn reconcile(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut BTreeMap<PathBuf, bool>,
    specs: &[WatchSpec],
) -> Result<bool, String> {
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
    let transitioned = desired != *active;
    for (path, recursive) in &desired {
        if !active.contains_key(path) {
            admit(watcher, path, *recursive)?;
            active.insert(path.clone(), *recursive);
        }
    }
    let removed: Vec<_> = active
        .iter()
        .filter(|(path, recursive)| desired.get(*path) != Some(*recursive))
        .map(|(path, recursive)| (path.clone(), *recursive))
        .collect();
    for (path, recursive) in removed {
        remove(watcher, &path)?;
        active.retain(|child, child_recursive| {
            !(PathCase::native().same(child, &path)
                || (recursive && PathCase::native().within(child, &path))
                || (*child_recursive && PathCase::native().within(&path, child)))
        });
    }
    for (path, recursive) in desired {
        if active.get(&path) == Some(&recursive) {
            continue;
        }
        admit(watcher, &path, recursive)?;
        active.insert(path, recursive);
    }
    Ok(transitioned)
}

fn admit(
    watcher: &mut (impl Watcher + ?Sized),
    path: &std::path::Path,
    recursive: bool,
) -> Result<(), String> {
    let metadata = path.symlink_metadata().map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "watch admission refused a symlink: {}",
            path.display()
        ));
    }
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher
        .watch(path, mode)
        .map_err(|error| format!("watch {}: {error}", path.display()))
}

pub(crate) fn invalidate(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut BTreeMap<PathBuf, bool>,
    changed: &[PathBuf],
) -> Result<(), String> {
    invalidate_with_case(watcher, active, changed, PathCase::native())
}

pub(crate) fn invalidate_with_case(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut BTreeMap<PathBuf, bool>,
    changed: &[PathBuf],
    case: PathCase,
) -> Result<(), String> {
    let invalidated: Vec<_> = active
        .keys()
        .filter(|root| changed.iter().any(|path| case.within(root, path)))
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
