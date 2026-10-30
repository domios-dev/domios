use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvError};
use std::sync::Arc;

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub(super) struct FileEvents {
    watcher: RecommendedWatcher,
    receiver: Receiver<()>,
    roots: Vec<PathBuf>,
    watched: HashMap<PathBuf, RecursiveMode>,
    roots_changed: Arc<AtomicBool>,
}

fn relevant(event: &Event, roots: &[PathBuf]) -> bool {
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
                .any(|root| path.starts_with(root) || root.starts_with(path))
        })
}

impl FileEvents {
    pub(super) fn new(roots: Vec<PathBuf>) -> notify::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let event_roots = roots.clone();
        let roots_changed = Arc::new(AtomicBool::new(false));
        let changed = roots_changed.clone();
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let wake = match event {
                    Ok(event) if relevant(&event, &event_roots) => {
                        if event.paths.is_empty()
                            || event
                                .paths
                                .iter()
                                .any(|path| event_roots.iter().any(|root| root.starts_with(path)))
                        {
                            changed.store(true, Ordering::Release);
                        }
                        true
                    }
                    Ok(_) => false,
                    Err(error) => {
                        log::error!("[plugin-watcher] filesystem event failed: {error}");
                        changed.store(true, Ordering::Release);
                        true
                    }
                };
                if wake {
                    let _ = sender.try_send(());
                }
            },
            Config::default().with_follow_symlinks(false),
        )?;
        let mut events = Self {
            watcher,
            receiver,
            roots,
            watched: HashMap::new(),
            roots_changed,
        };
        events.refresh()?;
        Ok(events)
    }

    pub(super) fn wait(&self) -> Result<(), RecvError> {
        self.receiver.recv()
    }

    pub(super) fn refresh(&mut self) -> notify::Result<()> {
        if self.roots_changed.swap(false, Ordering::AcqRel) {
            let invalidated: Vec<_> = self
                .watched
                .keys()
                .filter(|path| self.roots.iter().any(|root| path.starts_with(root)))
                .cloned()
                .collect();
            for path in invalidated {
                self.watched.remove(&path);
                let _ = self.watcher.unwatch(&path);
            }
        }
        let desired = match desired_watches(&self.roots) {
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
