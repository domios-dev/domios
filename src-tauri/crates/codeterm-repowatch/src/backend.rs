use notify::{
    Config as NotifyConfig, Event, EventHandler, RecommendedWatcher, RecursiveMode, Watcher,
    WatcherKind,
};
use notify_debouncer_mini::{new_debouncer_opt, Config as DebouncerConfig, DebounceEventResult};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Coalescing window for a burst an event already started. Elapsed time only
/// delays delivery; it never discovers a change, and absence is never evidence.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSpec {
    pub path: PathBuf,
    pub recursive: bool,
    /// Absent in a fresh repository; its parent's non-recursive watch covers it.
    pub optional: bool,
    pub partitioned: bool,
}

impl WatchSpec {
    pub fn worktree(path: PathBuf) -> Self {
        Self {
            partitioned: true,
            ..Self::shallow(path)
        }
    }

    pub fn recursive(path: PathBuf) -> Self {
        Self {
            path,
            recursive: true,
            optional: false,
            partitioned: false,
        }
    }

    pub fn shallow(path: PathBuf) -> Self {
        Self {
            path,
            recursive: false,
            optional: false,
            partitioned: false,
        }
    }

    pub fn optional_recursive(path: PathBuf) -> Self {
        Self {
            path,
            recursive: true,
            optional: true,
            partitioned: false,
        }
    }
}

/// What a live watch reports. `Rescan` and `Failed` both mean the watch can no
/// longer be trusted, so both must reach the UI rather than the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSignal {
    Batch(Vec<PathBuf>),
    CoverageChanged,
    Rescan,
    Failed(String),
}

pub type BatchHandler = Box<dyn FnMut(WatchSignal) + Send + 'static>;
pub type RescanSink = Arc<dyn Fn() + Send + Sync>;

/// A live OS watch. Dropping it releases every backend handle it owns.
pub trait ActiveWatch: Send {
    fn update(&mut self, specs: &[WatchSpec]) -> Result<(), String>;
}

/// The construction seam. A test injects a backend that refuses or one that
/// hands back its handler, which is the only deterministic way to drive both.
pub trait WatchBackend: Send + Sync {
    fn build(
        &self,
        specs: &[WatchSpec],
        handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String>;
}

/// `notify` flags an event when events may have been missed. The debouncer
/// destroys that flag, so it is intercepted upstream of it.
pub fn is_rescan(event: &notify::Result<Event>) -> bool {
    matches!(event, Ok(e) if e.need_rescan())
}

pub fn is_observation_only(event: &notify::Result<Event>) -> bool {
    matches!(event, Ok(e) if matches!(e.kind, notify::EventKind::Access(_)))
}

/// Sits between the backend and the debouncer, lifting the rescan flag out
/// before the zero-path event is dropped, then forwarding untouched.
pub struct RescanRelay<F: EventHandler> {
    inner: F,
    sink: Option<RescanSink>,
    replacements: Option<Arc<dyn Fn(Vec<PathBuf>) + Send + Sync>>,
}

impl<F: EventHandler> RescanRelay<F> {
    pub fn new(inner: F, sink: Option<RescanSink>) -> Self {
        Self {
            inner,
            sink,
            replacements: None,
        }
    }
}

impl<F: EventHandler> EventHandler for RescanRelay<F> {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        if let Ok(event) = &event {
            if matches!(
                event.kind,
                notify::EventKind::Remove(_)
                    | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
            ) {
                if let Some(replacements) = &self.replacements {
                    replacements(event.paths.clone());
                }
            }
        }
        if is_rescan(&event) {
            if let Some(sink) = &self.sink {
                sink();
            }
        }
        if is_observation_only(&event) {
            return;
        }
        self.inner.handle_event(event);
    }
}

thread_local! {
    /// Read by `RescanWatcher::new`, which the debouncer calls synchronously on
    /// this same thread while `build` is still on the stack.
    static RESCAN_SINK: RefCell<Option<RescanSink>> = RefCell::new(None);
    static REPLACEMENT_SINK: RefCell<Option<Arc<dyn Fn(Vec<PathBuf>) + Send + Sync>>> = RefCell::new(None);
}

struct RescanWatcher {
    inner: RecommendedWatcher,
}

impl Watcher for RescanWatcher {
    fn new<F: EventHandler>(event_handler: F, config: NotifyConfig) -> notify::Result<Self> {
        let sink = RESCAN_SINK.with(|slot| slot.borrow_mut().take());
        let mut relay = RescanRelay::new(event_handler, sink);
        relay.replacements = REPLACEMENT_SINK.with(|slot| slot.borrow_mut().take());
        let inner = RecommendedWatcher::new(relay, admission_config(config))?;
        Ok(Self { inner })
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(notify::Error::generic("watch admission refused a symlink")
                .add_path(path.to_path_buf()));
        }
        self.inner.watch(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.inner.unwatch(path)
    }

    fn configure(&mut self, option: NotifyConfig) -> notify::Result<bool> {
        self.inner.configure(option)
    }

    fn kind() -> WatcherKind {
        RecommendedWatcher::kind()
    }
}

pub(crate) fn admission_config(config: NotifyConfig) -> NotifyConfig {
    config.with_follow_symlinks(false)
}

pub struct NotifyBackend;

enum Command {
    Set(Vec<WatchSpec>, std::sync::mpsc::Sender<Result<(), String>>),
    Signal(WatchSignal),
    Replaced(Vec<PathBuf>),
    Stop,
}

struct NotifyWatch {
    commands: std::sync::mpsc::Sender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ActiveWatch for NotifyWatch {
    fn update(&mut self, specs: &[WatchSpec]) -> Result<(), String> {
        let (reply, result) = std::sync::mpsc::channel();
        self.commands
            .send(Command::Set(specs.to_vec(), reply))
            .map_err(|e| e.to_string())?;
        result.recv().map_err(|e| e.to_string())?
    }
}

impl Drop for NotifyWatch {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl WatchBackend for NotifyBackend {
    fn build(
        &self,
        specs: &[WatchSpec],
        mut handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        let (dispatch, signals) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("repository-watch-dispatch".into())
            .spawn(move || {
                while let Ok(signal) = signals.recv() {
                    handler(signal);
                }
            })
            .map_err(|error| error.to_string())?;
        let (commands, incoming) = std::sync::mpsc::channel();
        let events = commands.clone();
        let (ready, started) = std::sync::mpsc::channel();
        let initial = specs.to_vec();
        let thread = std::thread::Builder::new()
            .name("repository-watch-set".into())
            .spawn(move || {
                let on_rescan = events.clone();
                let on_replacement = events.clone();
                REPLACEMENT_SINK.with(|slot| {
                    *slot.borrow_mut() = Some(Arc::new(move |paths| {
                        let _ = on_replacement.send(Command::Replaced(paths));
                    }))
                });
                RESCAN_SINK.with(|slot| {
                    *slot.borrow_mut() = Some(Arc::new(move || {
                        let _ = on_rescan.send(Command::Signal(WatchSignal::Rescan));
                    }))
                });
                let built = new_debouncer_opt::<_, RescanWatcher>(
                    DebouncerConfig::default().with_timeout(DEBOUNCE),
                    move |result: DebounceEventResult| {
                        let signal = match result {
                            Ok(events) => {
                                let mut paths: Vec<_> =
                                    events.into_iter().map(|e| e.path).collect();
                                paths.sort();
                                paths.dedup();
                                WatchSignal::Batch(paths)
                            }
                            Err(error) => WatchSignal::Failed(error.to_string()),
                        };
                        let _ = events.send(Command::Signal(signal));
                    },
                );
                RESCAN_SINK.with(|slot| *slot.borrow_mut() = None);
                REPLACEMENT_SINK.with(|slot| *slot.borrow_mut() = None);
                let mut debouncer = match built {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = ready.send(Err(error.to_string()));
                        return;
                    }
                };
                let mut active = std::collections::BTreeMap::new();
                let mut specs = initial;
                let result =
                    super::subscriptions::reconcile(debouncer.watcher(), &mut active, &specs);
                let failed = result.is_err();
                if ready.send(result.map(|_| ())).is_err() || failed {
                    return;
                }
                while let Ok(command) = incoming.recv() {
                    match command {
                        Command::Stop => break,
                        Command::Set(updated, reply) => {
                            let result = super::subscriptions::reconcile(
                                debouncer.watcher(),
                                &mut active,
                                &updated,
                            );
                            if result.is_ok() {
                                specs = updated;
                            }
                            if matches!(result, Ok(true)) {
                                let _ = dispatch.send(WatchSignal::CoverageChanged);
                            }
                            if let Err(error) = &result {
                                let _ = dispatch.send(WatchSignal::Failed(error.clone()));
                            }
                            let _ = reply.send(result.map(|_| ()));
                        }
                        Command::Replaced(paths) => {
                            for signal in refresh_coverage(
                                debouncer.watcher(),
                                &mut active,
                                &specs,
                                &paths,
                                true,
                            ) {
                                let _ = dispatch.send(signal);
                            }
                        }
                        Command::Signal(signal) => {
                            if let WatchSignal::Batch(paths) = &signal {
                                for signal in refresh_coverage(
                                    debouncer.watcher(),
                                    &mut active,
                                    &specs,
                                    paths,
                                    false,
                                ) {
                                    let _ = dispatch.send(signal);
                                }
                            }
                            let _ = dispatch.send(signal);
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let watch = NotifyWatch {
            commands,
            thread: Some(thread),
        };
        started.recv().map_err(|e| e.to_string())??;
        Ok(Box::new(watch))
    }
}

pub(crate) fn refresh_coverage(
    watcher: &mut (impl Watcher + ?Sized),
    active: &mut std::collections::BTreeMap<PathBuf, bool>,
    specs: &[WatchSpec],
    paths: &[PathBuf],
    replaced: bool,
) -> Vec<WatchSignal> {
    if !replaced && !super::subscriptions::topology_changed(specs, active, paths) {
        return Vec::new();
    }
    let result = (|| {
        let mut changed = super::subscriptions::reconcile(watcher, active, specs)?;
        if replaced {
            let before = active.len();
            super::subscriptions::invalidate(watcher, active, paths)?;
            changed |= before != active.len();
            changed |= super::subscriptions::reconcile(watcher, active, specs)?;
        }
        Ok::<_, String>(changed)
    })();
    match result {
        Ok(true) => vec![WatchSignal::CoverageChanged],
        Ok(false) => Vec::new(),
        Err(error) => vec![WatchSignal::Failed(error)],
    }
}
