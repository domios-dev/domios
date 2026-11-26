use codeterm_util::watch_plan::PathCase;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

trait PathWatcher: Send {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()>;
    fn unwatch(&mut self, path: &Path) -> notify::Result<()>;
}

#[cfg(test)]
pub(super) type HoldsPath = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

impl PathWatcher for RecommendedWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        Watcher::watch(self, path, mode)
    }
    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        Watcher::unwatch(self, path)
    }
}

#[derive(Clone)]
struct EventState {
    roots: Arc<Vec<PathBuf>>,
    all_changed: Arc<AtomicBool>,
    invalidated: Arc<Mutex<Vec<PathBuf>>>,
    sender: mpsc::SyncSender<()>,
}

impl EventState {
    fn publish(&self, event: notify::Result<Event>) {
        let wake = match event {
            Ok(event) if relevant(&event, &self.roots) => {
                if event.paths.is_empty()
                    || event.paths.iter().any(|path| {
                        self.roots
                            .iter()
                            .any(|root| PathCase::native().within(root, path))
                    })
                {
                    self.all_changed.store(true, Ordering::Release);
                }
                if matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(notify::event::ModifyKind::Name(_))
                ) {
                    self.invalidated.lock().extend(event.paths);
                }
                true
            }
            Ok(_) => false,
            Err(error) => {
                log::error!("[plugin-watcher] filesystem event failed: {error}");
                self.all_changed.store(true, Ordering::Release);
                true
            }
        };
        if wake {
            let _ = self.sender.try_send(());
        }
    }
}

pub(super) struct FileEvents {
    watcher: Box<dyn PathWatcher>,
    receiver: Receiver<()>,
    watched: HashMap<PathBuf, RecursiveMode>,
    state: EventState,
}

fn relevant(event: &Event, roots: &[PathBuf]) -> bool {
    relevant_with_case(event, roots, PathCase::native())
}

fn relevant_with_case(event: &Event, roots: &[PathBuf], case: PathCase) -> bool {
    if matches!(event.kind, EventKind::Access(_))
        && !matches!(
            event.kind,
            EventKind::Access(AccessKind::Close(AccessMode::Write))
        )
    {
        return false;
    }
    event.paths.is_empty()
        || event.paths.iter().any(|path| {
            roots
                .iter()
                .any(|root| case.within(path, root) || case.within(root, path))
        })
}

fn is_temp_install_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().ends_with(".tmp-install")
}

fn is_already_released(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::WatchNotFound | notify::ErrorKind::PathNotFound => true,
        notify::ErrorKind::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
        ),
        _ => false,
    }
}

fn unwatch_idempotent(watcher: &mut dyn PathWatcher, path: &Path) -> notify::Result<()> {
    match watcher.unwatch(path) {
        Ok(()) => Ok(()),
        Err(error) if is_already_released(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

impl FileEvents {
    pub(super) fn take_receiver(&mut self) -> Receiver<()> {
        std::mem::replace(&mut self.receiver, mpsc::sync_channel(1).1)
    }

    pub(super) fn release_bundle(&mut self, destination: &Path) -> notify::Result<()> {
        let destination = canonical_root(destination).map_err(notify::Error::io)?;
        let name = destination
            .file_name()
            .ok_or_else(|| notify::Error::generic("Plugin bundle has no name"))?
            .to_string_lossy();
        let staging = destination.with_file_name(format!("{name}.tmp-install"));
        let backup = destination.with_file_name(format!("{name}.rollback.tmp-install"));
        let paths: Vec<_> = self
            .watched
            .keys()
            .filter(|path| {
                [&destination, &staging, &backup]
                    .iter()
                    .any(|bundle| PathCase::native().within(path, bundle))
            })
            .cloned()
            .collect();
        for path in paths {
            self.watched.remove(&path);
            unwatch_idempotent(self.watcher.as_mut(), &path)?;
        }
        let parent = destination
            .parent()
            .ok_or_else(|| notify::Error::generic("Plugin bundle has no parent"))?;
        if self.watched.remove(parent).is_some() {
            unwatch_idempotent(self.watcher.as_mut(), parent)?;
        }
        // Windows watch acknowledges queued removals; unwatch and Drop alone do not.
        if let Some(barrier) = parent.ancestors().find(|path| path.is_dir()) {
            if barrier != parent && self.watched.remove(barrier).is_some() {
                unwatch_idempotent(self.watcher.as_mut(), barrier)?;
            }
            self.watcher.watch(barrier, RecursiveMode::NonRecursive)?;
            self.watched
                .insert(barrier.to_path_buf(), RecursiveMode::NonRecursive);
        }
        Ok(())
    }

    pub(super) fn rearm_after_install(&mut self, destination: &Path) -> notify::Result<()> {
        if let Ok(destination) = canonical_root(destination) {
            if let Some(parent) = destination.parent() {
                if self.watched.remove(parent).is_some() {
                    let _ = unwatch_idempotent(self.watcher.as_mut(), parent);
                }
            }
        }
        self.refresh()
    }

    pub(super) fn new(roots: Vec<PathBuf>) -> notify::Result<Self> {
        let roots = roots
            .iter()
            .map(|root| canonical_root(root).map_err(notify::Error::io))
            .collect::<notify::Result<Vec<_>>>()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let state = EventState {
            roots: Arc::new(roots),
            all_changed: Arc::new(AtomicBool::new(false)),
            invalidated: Arc::new(Mutex::new(Vec::new())),
            sender,
        };
        let callback = state.clone();
        let watcher = RecommendedWatcher::new(
            move |event| callback.publish(event),
            Config::default().with_follow_symlinks(false),
        )?;
        let mut events = Self {
            watcher: Box::new(watcher),
            receiver,
            watched: HashMap::new(),
            state,
        };
        events.refresh()?;
        Ok(events)
    }

    pub(super) fn refresh(&mut self) -> notify::Result<()> {
        let mut changed = std::mem::take(&mut *self.state.invalidated.lock());
        if self.state.all_changed.swap(false, Ordering::AcqRel) {
            changed.extend(self.state.roots.iter().cloned());
        }
        let invalidated: Vec<_> = self
            .watched
            .keys()
            .filter(|path| {
                changed
                    .iter()
                    .any(|changed| PathCase::native().within(path, changed))
            })
            .cloned()
            .collect();
        for path in invalidated {
            self.watched.remove(&path);
            let _ = self.watcher.unwatch(&path);
        }
        let desired = match desired_watches(&self.state.roots) {
            Ok(desired) => desired,
            Err(error) => {
                for path in self.watched.keys() {
                    let _ = self.watcher.unwatch(path);
                }
                self.watched.clear();
                return Err(error);
            }
        };
        for (path, mode) in &self.watched {
            if desired.get(path) != Some(mode) {
                let _ = self.watcher.unwatch(path);
            }
        }
        self.watched
            .retain(|path, mode| desired.get(path) == Some(mode));
        for (path, mode) in desired {
            if self.watched.get(&path) != Some(&mode) {
                match self.watcher.watch(&path, mode) {
                    Ok(()) => {
                        self.watched.insert(path, mode);
                    }
                    Err(_) if path.try_exists().is_ok_and(|exists| !exists) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }
}

fn canonical_root(root: &Path) -> std::io::Result<PathBuf> {
    let absolute = std::path::absolute(root)?;
    for ancestor in absolute.ancestors() {
        match ancestor.canonicalize() {
            Ok(canonical) => {
                let suffix = absolute
                    .strip_prefix(ancestor)
                    .map_err(std::io::Error::other)?;
                return Ok(canonical.join(suffix));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Plugin watch root has no existing ancestor",
    ))
}

fn desired_watches(roots: &[PathBuf]) -> notify::Result<HashMap<PathBuf, RecursiveMode>> {
    let mut desired = HashMap::new();
    for root in roots {
        if let Some(parent) = root
            .parent()
            .and_then(|parent| parent.ancestors().find(|p| p.is_dir()))
        {
            desired
                .entry(parent.to_path_buf())
                .or_insert(RecursiveMode::NonRecursive);
        }
        if root.is_dir() {
            for watch in
                codeterm_util::watch_plan::covering_set_excluding(root, true, is_temp_install_name)
                    .map_err(|error| notify::Error::generic(&error))?
            {
                let mode = if watch.recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                desired.insert(watch.path, mode);
            }
        }
    }
    if desired.len() > codeterm_util::watch_plan::MAX_WATCHES {
        let error = format!("plugin watch subscription limit exceeded: {} directories, maximum {}; watching refused", desired.len(), codeterm_util::watch_plan::MAX_WATCHES);
        log::warn!("{error}");
        return Err(notify::Error::generic(&error));
    }
    Ok(desired)
}

#[cfg(test)]
pub(super) mod tests;
