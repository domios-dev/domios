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
            for root in &self.roots {
                if self.watched.remove(root).is_some() {
                    let _ = self.watcher.unwatch(root);
                }
            }
        }
        let mut desired = HashMap::new();
        for root in &self.roots {
            if let Some(parent) = root
                .parent()
                .and_then(|parent| parent.ancestors().find(|p| p.is_dir()))
            {
                desired
                    .entry(parent.to_path_buf())
                    .or_insert(RecursiveMode::NonRecursive);
            }
            if root.is_dir() {
                desired.insert(root.clone(), RecursiveMode::Recursive);
            }
        }
        for (path, mode) in &desired {
            if self.watched.get(path) != Some(mode) {
                self.watcher.watch(path, *mode)?;
            }
        }
        for path in self
            .watched
            .keys()
            .filter(|path| !desired.contains_key(*path))
        {
            let _ = self.watcher.unwatch(path);
        }
        self.watched = desired;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn plugin_reads_and_unrelated_files_do_not_schedule_reloads() {
        let roots = vec![PathBuf::from("/plugins")];
        let read = Event::new(EventKind::Access(AccessKind::Read))
            .add_path("/plugins/provider/plugin.js".into());
        let unrelated = Event::new(EventKind::Any).add_path("/sessions/output.log".into());
        let changed = Event::new(EventKind::Any).add_path("/plugins/provider/plugin.js".into());
        assert!(!relevant(&read, &roots));
        assert!(!relevant(&unrelated, &roots));
        assert!(relevant(&changed, &roots));
    }

    #[test]
    fn filesystem_changes_wake_the_reconciler_without_a_polling_tick() {
        let directory = tempfile::tempdir().unwrap();
        let events = FileEvents::new(vec![directory.path().to_path_buf()]).unwrap();
        let started = std::time::Instant::now();
        std::fs::write(directory.path().join("plugin.json"), "{}").unwrap();
        events
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        eprintln!("plugin filesystem wake: {:?}", started.elapsed());
    }

    #[test]
    fn a_missing_plugin_root_is_observed_through_its_existing_parent() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let mut events = FileEvents::new(vec![root.clone()]).unwrap();
        std::fs::create_dir(&root).unwrap();
        events
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        events.refresh().unwrap();
        assert_eq!(events.watched.get(&root), Some(&RecursiveMode::Recursive));
        std::fs::remove_dir(&root).unwrap();
        events
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        events.refresh().unwrap();
        assert!(!events.watched.contains_key(&root));
    }
}
