use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::OnceLock;

pub(super) fn affects_source(event: &Event, source: &Path) -> bool {
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

pub(super) struct WatchGuard {
    commands: Sender<Command>,
    id: u64,
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
        let affected: Vec<_> = self
            .subscribers
            .iter()
            .filter_map(|(id, subscriber)| {
                (subscriber.root == root
                    && event
                        .as_ref()
                        .map_or(true, |event| affects_source(event, &subscriber.source)))
                .then_some(*id)
            })
            .collect();
        let invalidated = event.is_err()
            || event.as_ref().is_ok_and(|event| {
                matches!(
                    event.kind,
                    EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
                ) && event.paths.iter().any(|path| root.starts_with(path))
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

pub(super) fn watch(source: &Path, wake: SyncSender<()>) -> Result<WatchGuard, String> {
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
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    fn receive(events: &Receiver<()>) {
        events.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    fn roots(guard: &WatchGuard, directory: &Path) -> Vec<(PathBuf, usize)> {
        let (reply, result) = mpsc::channel();
        guard.commands.send(Command::Inspect(reply)).unwrap();
        result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .into_iter()
            .filter(|(root, _)| root.starts_with(directory))
            .collect()
    }

    fn await_root(guard: &WatchGuard, events: &Receiver<()>, directory: &Path, expected: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if roots(guard, directory) == vec![(expected.to_path_buf(), 1)] {
                return;
            }
            events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
        }
    }

    #[test]
    fn append_and_materialization_wake_only_the_claimed_source() {
        let source = Path::new("sessions/own/turn.jsonl");
        assert!(affects_source(
            &Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(source.into()),
            source
        ));
        assert!(affects_source(
            &Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                .add_path("sessions/own".into()),
            source
        ));
        assert!(!affects_source(
            &Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path("sessions/foreign/turn.jsonl".into()),
            source
        ));
        assert!(!affects_source(
            &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(source.into()),
            source
        ));
    }

    #[test]
    fn sibling_subscribers_share_one_native_watcher_and_drop_independently() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let first = directory.join("first.jsonl");
        let second = directory.join("second.jsonl");
        std::fs::write(&first, b"").unwrap();
        std::fs::write(&second, b"").unwrap();
        let (wake_first, events_first) = mpsc::sync_channel(1);
        let (wake_second, events_second) = mpsc::sync_channel(1);
        let first_guard = watch(&first, wake_first).unwrap();
        let second_guard = watch(&second, wake_second).unwrap();
        assert_eq!(
            roots(&first_guard, &directory),
            vec![(directory.clone(), 2)]
        );
        std::fs::write(&first, b"first\n").unwrap();
        receive(&events_first);
        assert!(events_second.try_recv().is_err());
        drop(first_guard);
        assert_eq!(
            roots(&second_guard, &directory),
            vec![(directory.clone(), 1)]
        );
        std::fs::write(&second, b"second\n").unwrap();
        receive(&events_second);
        let commands = second_guard.commands.clone();
        drop(second_guard);
        let (reply, result) = mpsc::channel();
        commands.send(Command::Inspect(reply)).unwrap();
        assert!(result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .iter()
            .all(|(root, _)| !root.starts_with(&directory)));
    }

    #[test]
    fn missing_source_descends_and_survives_file_recreation() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let session = directory.join("sessions").join("own");
        let source = session.join("turn.jsonl");
        let (wake, events) = mpsc::sync_channel(1);
        let guard = watch(&source, wake).unwrap();
        assert_eq!(roots(&guard, &directory), vec![(directory.clone(), 1)]);
        std::fs::create_dir(directory.join("sessions")).unwrap();
        await_root(&guard, &events, &directory, &directory.join("sessions"));
        std::fs::create_dir(&session).unwrap();
        await_root(&guard, &events, &directory, &session);
        while events.try_recv().is_ok() {}
        std::fs::write(&source, b"first\n").unwrap();
        receive(&events);
        std::fs::remove_file(&source).unwrap();
        receive(&events);
        std::fs::write(&source, b"replacement\n").unwrap();
        receive(&events);
        assert_eq!(roots(&guard, &directory), vec![(session, 1)]);
    }

    #[test]
    fn materialized_branches_converge_on_the_same_shared_root() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let session = directory.join("sessions");
        let (wake_first, events_first) = mpsc::sync_channel(1);
        let (wake_second, events_second) = mpsc::sync_channel(1);
        let first = watch(&session.join("first.jsonl"), wake_first).unwrap();
        let second = watch(&session.join("second.jsonl"), wake_second).unwrap();
        std::fs::create_dir(&session).unwrap();
        receive(&events_first);
        receive(&events_second);
        assert_eq!(roots(&first, &directory), vec![(session, 2)]);
        drop(second);
    }

    #[test]
    #[cfg(unix)]
    fn filesystem_append_wakes_through_a_directory_alias() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let source = alias.join("own.jsonl");
        std::fs::write(&source, b"").unwrap();
        let (wake, events) = mpsc::sync_channel(1);
        let _watcher = watch(&source, wake).unwrap();
        std::fs::write(&source, b"opaque-turn\n").unwrap();
        receive(&events);
    }

    #[test]
    fn filesystem_append_wakes_without_a_polling_clock() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("own.jsonl");
        std::fs::write(&source, b"").unwrap();
        let (wake, events) = mpsc::sync_channel(1);
        let _watcher = watch(&source, wake).unwrap();
        std::fs::write(&source, b"opaque-turn\n").unwrap();
        receive(&events);
    }
}
