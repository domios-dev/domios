use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, OnceLock};

pub(super) fn affects_source(event: &Event, source: &Path) -> bool {
    if event.need_rescan() {
        return true;
    }
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|path| path == source || source.starts_with(path))
}

struct Subscriber {
    source: PathBuf,
    root: PathBuf,
    wake: SyncSender<()>,
    armed: Arc<AtomicBool>,
}

enum Command {
    Subscribe(
        PathBuf,
        SyncSender<()>,
        Arc<AtomicBool>,
        Sender<Result<u64, String>>,
    ),
    Unsubscribe(u64),
    Event(PathBuf, u64, notify::Result<Event>),
    #[cfg(test)]
    Inspect(Sender<Vec<(PathBuf, usize)>>),
}

pub(super) struct WatchGuard {
    commands: Sender<Command>,
    id: u64,
    armed: Arc<AtomicBool>,
}

impl WatchGuard {
    /// False once the subscription lost its native watcher and could not be re-rooted.
    pub(super) fn armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Unsubscribe(self.id));
    }
}

struct RootWatch {
    _watcher: RecommendedWatcher,
    generation: u64,
}

#[derive(Default)]
struct Registry {
    roots: HashMap<PathBuf, RootWatch>,
    subscribers: HashMap<u64, Subscriber>,
    next_id: u64,
    generation: u64,
    logged_counts: (usize, usize),
}

fn nearest_directory(source: &Path) -> Result<PathBuf, String> {
    source
        .ancestors()
        .skip(1)
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("no watchable ancestor for {}", source.display()))
}

fn canonical_source(source: &Path) -> Result<PathBuf, String> {
    let root = nearest_directory(source)?;
    let relative = source
        .strip_prefix(&root)
        .map_err(|error| error.to_string())?;
    Ok(root
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join(relative))
}

impl Registry {
    fn ensure_root(&mut self, root: &Path, commands: &Sender<Command>) -> Result<(), String> {
        if self.roots.contains_key(root) {
            return Ok(());
        }
        self.generation += 1;
        let generation = self.generation;
        let events = commands.clone();
        let event_root = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Command::Event(event_root.clone(), generation, event));
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(root, RecursiveMode::NonRecursive)
            .map_err(|error| error.to_string())?;
        self.roots.insert(
            root.to_path_buf(),
            RootWatch {
                _watcher: watcher,
                generation,
            },
        );
        Ok(())
    }

    fn locate(&mut self, source: &Path, commands: &Sender<Command>) -> Result<PathBuf, String> {
        let mut root = nearest_directory(source)?;
        loop {
            self.ensure_root(&root, commands)?;
            let current = nearest_directory(source)?;
            if current == root {
                return Ok(root);
            }
            root = current;
        }
    }

    fn prune(&mut self) {
        self.roots.retain(|root, _| {
            self.subscribers
                .values()
                .any(|subscriber| subscriber.root == *root)
        });
        let counts = (self.roots.len(), self.subscribers.len());
        if counts != self.logged_counts {
            log::debug!(
                "[tab_chat.watch] native_watchers={} subscribers={}",
                counts.0,
                counts.1
            );
            self.logged_counts = counts;
        }
    }

    fn subscribe(
        &mut self,
        source: PathBuf,
        wake: SyncSender<()>,
        armed: Arc<AtomicBool>,
        commands: &Sender<Command>,
    ) -> Result<u64, String> {
        let result = self.locate(&source, commands).map(|root| {
            self.next_id += 1;
            self.subscribers.insert(
                self.next_id,
                Subscriber {
                    source,
                    root,
                    wake,
                    armed,
                },
            );
            self.next_id
        });
        self.prune();
        result
    }

    fn affected_subscribers(&self, root: &Path, event: &notify::Result<Event>) -> Vec<u64> {
        self.subscribers
            .iter()
            .filter_map(|(id, subscriber)| {
                (subscriber.root == root
                    && event
                        .as_ref()
                        .map_or(true, |event| affects_source(event, &subscriber.source)))
                .then_some(*id)
            })
            .collect()
    }

    fn event(
        &mut self,
        root: PathBuf,
        generation: u64,
        event: notify::Result<Event>,
        commands: &Sender<Command>,
    ) {
        if !self
            .roots
            .get(&root)
            .is_some_and(|watch| watch.generation == generation)
        {
            return;
        }
        let affected = self.affected_subscribers(&root, &event);
        let invalidated = event.is_err()
            || event.as_ref().is_ok_and(|event| {
                event.need_rescan()
                    || (matches!(
                        event.kind,
                        EventKind::Remove(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    ) && event.paths.iter().any(|path| root.starts_with(path)))
            });
        if invalidated {
            self.roots.remove(&root);
        }
        let mut changed = invalidated;
        for id in affected {
            let source = self.subscribers[&id].source.clone();
            if invalidated || source.parent() != Some(root.as_path()) || event.is_err() {
                changed = true;
                let located = self.locate(&source, commands);
                if let Some(subscriber) = self.subscribers.get_mut(&id) {
                    subscriber.armed.store(located.is_ok(), Ordering::Release);
                    match located {
                        Ok(current) => subscriber.root = current,
                        Err(error) => log::debug!(
                            "[tab_chat.watch] rearm {} failed: {error}",
                            source.display()
                        ),
                    }
                }
            }
            if let Some(subscriber) = self.subscribers.get(&id) {
                let _ = subscriber.wake.try_send(());
            }
        }
        if changed {
            self.prune();
        }
    }
}

fn start_registry() -> Result<Sender<Command>, String> {
    let (commands, incoming) = mpsc::channel();
    let events = commands.clone();
    std::thread::Builder::new()
        .name("chat-file-watches".into())
        .spawn(move || {
            let mut registry = Registry::default();
            while let Ok(command) = incoming.recv() {
                match command {
                    Command::Subscribe(source, wake, armed, reply) => {
                        let result = registry.subscribe(source, wake, armed, &events);
                        if let Err(mpsc::SendError(Ok(id))) = reply.send(result) {
                            registry.subscribers.remove(&id);
                            registry.prune();
                        }
                    }
                    Command::Unsubscribe(id) => {
                        registry.subscribers.remove(&id);
                        registry.prune();
                    }
                    Command::Event(root, generation, event) => {
                        registry.event(root, generation, event, &events)
                    }
                    #[cfg(test)]
                    Command::Inspect(reply) => {
                        let _ = reply.send(
                            registry
                                .roots
                                .keys()
                                .map(|root| {
                                    (
                                        root.clone(),
                                        registry
                                            .subscribers
                                            .values()
                                            .filter(|subscriber| subscriber.root == *root)
                                            .count(),
                                    )
                                })
                                .collect(),
                        );
                    }
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(commands)
}

pub(super) fn watch(source: &Path, wake: SyncSender<()>) -> Result<WatchGuard, String> {
    static REGISTRY: OnceLock<Result<Sender<Command>, String>> = OnceLock::new();
    let commands = REGISTRY
        .get_or_init(start_registry)
        .as_ref()
        .map_err(Clone::clone)?
        .clone();
    let (reply, result) = mpsc::channel();
    let armed = Arc::new(AtomicBool::new(true));
    commands
        .send(Command::Subscribe(
            canonical_source(source)?,
            wake,
            armed.clone(),
            reply,
        ))
        .map_err(|error| error.to_string())?;
    let id = result.recv().map_err(|error| error.to_string())??;
    Ok(WatchGuard {
        commands,
        id,
        armed,
    })
}

#[cfg(test)]
mod tests;
