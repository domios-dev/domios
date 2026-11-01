use codeterm_util::watch_plan::PathCase;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvError};
use std::sync::Arc;

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

trait PathWatcher: Send {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()>;
    fn unwatch(&mut self, path: &Path) -> notify::Result<()>;
}

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

impl FileEvents {
    pub(super) fn new(roots: Vec<PathBuf>) -> notify::Result<Self> {
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

    pub(super) fn wait(&self) -> Result<(), RecvError> {
        self.receiver.recv()
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
        for (path, mode) in &desired {
            if self.watched.get(path) != Some(mode) {
                self.watcher.watch(path, *mode)?;
            }
        }
        self.watched = desired;
        Ok(())
    }
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
            for watch in codeterm_util::watch_plan::covering_set(root)
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
mod tests;
