use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, OnceLock};

pub(crate) fn affects_source(event: &Event, source: &Path) -> bool {
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
}

enum Command {
    Subscribe(PathBuf, SyncSender<()>, Sender<Result<u64, String>>),
    Unsubscribe(u64),
    Event(PathBuf, u64, notify::Result<Event>),
    #[cfg(test)]
    Inspect(Sender<Vec<(PathBuf, usize)>>),
}

pub(crate) struct WatchGuard {
    commands: Sender<Command>,
    id: u64,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Unsubscribe(self.id));
    }
}

struct RootWatch {
    _watcher: Box<dyn Send>,
    generation: u64,
}

struct Registry {
    roots: HashMap<PathBuf, RootWatch>,
    subscribers: HashMap<u64, Subscriber>,
    next_id: u64,
    generation: u64,
    logged_counts: (usize, usize),
    backend: Arc<dyn DirectoryBackend>,
}

trait DirectoryBackend: Send + Sync {
    fn nearest(&self, source: &Path) -> Result<PathBuf, String>;
    fn watch(
        &self,
        root: &Path,
        mode: RecursiveMode,
        generation: u64,
        commands: &Sender<Command>,
    ) -> Result<Box<dyn Send>, String>;
}

struct NativeBackend;

impl DirectoryBackend for NativeBackend {
    fn nearest(&self, source: &Path) -> Result<PathBuf, String> {
        nearest_directory(source)
    }

    fn watch(
        &self,
        root: &Path,
        mode: RecursiveMode,
        generation: u64,
        commands: &Sender<Command>,
    ) -> Result<Box<dyn Send>, String> {
        let events = commands.clone();
        let event_root = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Command::Event(event_root.clone(), generation, event));
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(root, mode)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(watcher))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            roots: HashMap::new(),
            subscribers: HashMap::new(),
            next_id: 0,
            generation: 0,
            logged_counts: (0, 0),
            backend: Arc::new(NativeBackend),
        }
    }
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
        let watcher =
            self.backend
                .watch(root, RecursiveMode::NonRecursive, generation, commands)?;
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
        let mut root = self.backend.nearest(source)?;
        loop {
            self.ensure_root(&root, commands)?;
            let current = self.backend.nearest(source)?;
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
        commands: &Sender<Command>,
    ) -> Result<u64, String> {
        let result = self.locate(&source, commands).map(|root| {
            self.next_id += 1;
            self.subscribers
                .insert(self.next_id, Subscriber { source, root, wake });
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
                match self.locate(&source, commands) {
                    Ok(current) => {
                        if let Some(subscriber) = self.subscribers.get_mut(&id) {
                            subscriber.root = current;
                        }
                    }
                    Err(error) => log::debug!(
                        "[tab_chat.watch] rearm {} failed: {error}",
                        source.display()
                    ),
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
                    Command::Subscribe(source, wake, reply) => {
                        let result = registry.subscribe(source, wake, &events);
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

pub(crate) fn watch(source: &Path, wake: SyncSender<()>) -> Result<WatchGuard, String> {
    static REGISTRY: OnceLock<Result<Sender<Command>, String>> = OnceLock::new();
    let commands = REGISTRY
        .get_or_init(start_registry)
        .as_ref()
        .map_err(Clone::clone)?
        .clone();
    let (reply, result) = mpsc::channel();
    commands
        .send(Command::Subscribe(canonical_source(source)?, wake, reply))
        .map_err(|error| error.to_string())?;
    let id = result.recv().map_err(|error| error.to_string())??;
    Ok(WatchGuard { commands, id })
}

#[cfg(test)]
mod tests;
