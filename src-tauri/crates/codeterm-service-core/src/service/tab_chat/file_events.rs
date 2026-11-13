use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    Suspend(PathBuf, Sender<Result<(), String>>),
    Resume(PathBuf, Sender<()>),
    Event(PathBuf, u64, notify::Result<Event>),
    #[cfg(test)]
    Inspect(Sender<Vec<(PathBuf, usize)>>),
}

pub(crate) struct WatchGuard {
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
    _watcher: Box<dyn DirectoryStream>,
    generation: u64,
}

trait DirectoryStream: Send {
    fn stop(&mut self) -> Result<(), String>;
}

struct NativeStream {
    watcher: notify::RecommendedWatcher,
    root: PathBuf,
    stopped: bool,
}

impl DirectoryStream for NativeStream {
    fn stop(&mut self) -> Result<(), String> {
        if !self.stopped {
            self.watcher
                .unwatch(&self.root)
                .map_err(|error| error.to_string())?;
            // Windows unwatch only enqueues removal; watch acknowledges the preceding removals.
            #[cfg(windows)]
            {
                let data_root = crate::paths::data_dir();
                let barrier = data_root
                    .ancestors()
                    .find(|path| path.is_dir())
                    .ok_or("No directory for watcher release acknowledgement")?;
                self.watcher
                    .watch(barrier, RecursiveMode::NonRecursive)
                    .map_err(|error| error.to_string())?;
            }
            self.stopped = true;
        }
        Ok(())
    }
}

impl Drop for NativeStream {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            log::error!("[tab_chat.watch] release {}: {error}", self.root.display());
        }
    }
}

struct Registry {
    roots: HashMap<PathBuf, RootWatch>,
    subscribers: HashMap<u64, Subscriber>,
    next_id: u64,
    generation: u64,
    logged_counts: (usize, usize),
    backend: Arc<dyn DirectoryBackend>,
    suspended: HashMap<PathBuf, usize>,
}

trait DirectoryBackend: Send + Sync {
    fn nearest(&self, source: &Path) -> Result<PathBuf, String>;
    fn watch(
        &self,
        root: &Path,
        mode: RecursiveMode,
        generation: u64,
        commands: &Sender<Command>,
    ) -> Result<Box<dyn DirectoryStream>, String>;
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
    ) -> Result<Box<dyn DirectoryStream>, String> {
        let events = commands.clone();
        let event_root = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Command::Event(event_root.clone(), generation, event));
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(root, mode)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(NativeStream {
            watcher,
            root: root.to_path_buf(),
            stopped: false,
        }))
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
            suspended: HashMap::new(),
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
    fn suspend(&mut self, path: PathBuf, commands: &Sender<Command>) -> Result<(), String> {
        *self.suspended.entry(path.clone()).or_default() += 1;
        let roots: Vec<_> = self
            .roots
            .keys()
            .filter(|root| codeterm_util::watch_plan::PathCase::native().within(root, &path))
            .cloned()
            .collect();
        let result = roots
            .into_iter()
            .try_for_each(|root| {
                self.roots
                    .get_mut(&root)
                    .expect("existing root")
                    ._watcher
                    .stop()?;
                self.roots.remove(&root);
                Ok(())
            })
            .and_then(|()| self.relocate(&path, commands));
        if result.is_err() {
            self.resume(&path, commands);
        }
        result
    }
    fn resume(&mut self, path: &Path, commands: &Sender<Command>) {
        if let Some(count) = self.suspended.get_mut(path) {
            *count -= 1;
            if *count == 0 {
                self.suspended.remove(path);
            }
        }
        if let Err(error) = self.relocate(path, commands) {
            log::error!("[tab_chat.watch] restore bundle watches: {error}");
        }
        for subscriber in self.subscribers.values().filter(|subscriber| {
            codeterm_util::watch_plan::PathCase::native().within(&subscriber.source, path)
        }) {
            let _ = subscriber.wake.try_send(());
        }
    }
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
        let mut root = self.watch_root(source)?;
        loop {
            self.ensure_root(&root, commands)?;
            let current = self.watch_root(source)?;
            if current == root {
                return Ok(root);
            }
            root = current;
        }
    }

    fn watch_root(&self, source: &Path) -> Result<PathBuf, String> {
        let mut root = self.backend.nearest(source)?;
        while let Some(bundle) = self
            .suspended
            .keys()
            .find(|bundle| codeterm_util::watch_plan::PathCase::native().within(&root, bundle))
        {
            root = bundle
                .parent()
                .ok_or("Plugin bundle has no parent")?
                .to_path_buf();
        }
        Ok(root)
    }

    fn relocate(&mut self, path: &Path, commands: &Sender<Command>) -> Result<(), String> {
        let sources: Vec<_> = self
            .subscribers
            .iter()
            .filter(|(_, subscriber)| {
                codeterm_util::watch_plan::PathCase::native().within(&subscriber.source, path)
            })
            .map(|(id, subscriber)| (*id, subscriber.source.clone()))
            .collect();
        let mut result = Ok(());
        for (id, source) in sources {
            let located = self.locate(&source, commands);
            if let Some(subscriber) = self.subscribers.get_mut(&id) {
                subscriber.armed.store(located.is_ok(), Ordering::Release);
                match located {
                    Ok(root) => subscriber.root = root,
                    Err(error) => result = Err(error),
                }
            }
        }
        self.prune();
        result
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
                    Command::Suspend(path, reply) => {
                        let result = registry.suspend(path.clone(), &events);
                        let suspended = result.is_ok();
                        if reply.send(result).is_err() && suspended {
                            registry.resume(&path, &events);
                        }
                    }
                    Command::Resume(path, reply) => {
                        registry.resume(&path, &events);
                        let _ = reply.send(());
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

fn registry_commands() -> Result<Sender<Command>, String> {
    static REGISTRY: OnceLock<Result<Sender<Command>, String>> = OnceLock::new();
    Ok(REGISTRY
        .get_or_init(start_registry)
        .as_ref()
        .map_err(Clone::clone)?
        .clone())
}

pub(crate) struct BundleWatchScope {
    commands: Sender<Command>,
    path: PathBuf,
}

impl Drop for BundleWatchScope {
    fn drop(&mut self) {
        let (reply, result) = mpsc::channel();
        if self
            .commands
            .send(Command::Resume(self.path.clone(), reply))
            .is_ok()
        {
            let _ = result.recv();
        }
    }
}

pub(crate) fn suspend_bundle(path: &Path) -> Result<BundleWatchScope, String> {
    let commands = registry_commands()?;
    let path = canonical_source(&path.join("plugin.json"))?
        .parent()
        .ok_or("Plugin bundle has no parent")?
        .to_path_buf();
    let (reply, result) = mpsc::channel();
    commands
        .send(Command::Suspend(path.clone(), reply))
        .map_err(|error| error.to_string())?;
    result.recv().map_err(|error| error.to_string())??;
    Ok(BundleWatchScope { commands, path })
}

pub(crate) fn watch(source: &Path, wake: SyncSender<()>) -> Result<WatchGuard, String> {
    let commands = registry_commands()?;
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
