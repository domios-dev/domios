use notify::{
    Config as NotifyConfig, Event, EventHandler, RecommendedWatcher, RecursiveMode, Watcher,
    WatcherKind,
};
use notify_debouncer_mini::{
    new_debouncer_opt, Config as DebouncerConfig, DebounceEventResult, Debouncer,
};
use parking_lot::Mutex;
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
}

impl WatchSpec {
    pub fn recursive(path: PathBuf) -> Self {
        Self {
            path,
            recursive: true,
            optional: false,
        }
    }

    pub fn shallow(path: PathBuf) -> Self {
        Self {
            path,
            recursive: false,
            optional: false,
        }
    }

    pub fn optional_recursive(path: PathBuf) -> Self {
        Self {
            path,
            recursive: true,
            optional: true,
        }
    }
}

/// What a live watch reports. `Rescan` and `Failed` both mean the watch can no
/// longer be trusted, so both must reach the UI rather than the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSignal {
    Batch(Vec<PathBuf>),
    Rescan,
    Failed(String),
}

pub type BatchHandler = Box<dyn FnMut(WatchSignal) + Send + 'static>;
pub type RescanSink = Arc<dyn Fn() + Send + Sync>;

/// A live OS watch. Dropping it releases every backend handle it owns.
pub trait ActiveWatch: Send {}

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

/// Sits between the backend and the debouncer, lifting the rescan flag out
/// before the zero-path event is dropped, then forwarding untouched.
pub struct RescanRelay<F: EventHandler> {
    inner: F,
    sink: Option<RescanSink>,
}

impl<F: EventHandler> RescanRelay<F> {
    pub fn new(inner: F, sink: Option<RescanSink>) -> Self {
        Self { inner, sink }
    }
}

impl<F: EventHandler> EventHandler for RescanRelay<F> {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        if is_rescan(&event) {
            if let Some(sink) = &self.sink {
                sink();
            }
        }
        self.inner.handle_event(event);
    }
}

thread_local! {
    /// Read by `RescanWatcher::new`, which the debouncer calls synchronously on
    /// this same thread while `build` is still on the stack.
    static RESCAN_SINK: RefCell<Option<RescanSink>> = RefCell::new(None);
}

struct RescanWatcher {
    inner: RecommendedWatcher,
}

impl Watcher for RescanWatcher {
    fn new<F: EventHandler>(event_handler: F, config: NotifyConfig) -> notify::Result<Self> {
        let sink = RESCAN_SINK.with(|slot| slot.borrow_mut().take());
        let inner = RecommendedWatcher::new(RescanRelay::new(event_handler, sink), config)?;
        Ok(Self { inner })
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
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

pub struct NotifyBackend;

struct NotifyWatch {
    _debouncer: Debouncer<RescanWatcher>,
}

impl ActiveWatch for NotifyWatch {}

impl WatchBackend for NotifyBackend {
    fn build(
        &self,
        specs: &[WatchSpec],
        handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        let shared = Arc::new(Mutex::new(handler));
        let on_rescan = shared.clone();
        let sink: RescanSink = Arc::new(move || {
            let mut guard = on_rescan.lock();
            (&mut **guard)(WatchSignal::Rescan);
        });
        RESCAN_SINK.with(|slot| *slot.borrow_mut() = Some(sink));

        let batched = shared.clone();
        let config = DebouncerConfig::default().with_timeout(DEBOUNCE);
        let built = new_debouncer_opt::<_, RescanWatcher>(
            config,
            move |result: DebounceEventResult| {
                let signal = match result {
                    Ok(events) => {
                        let mut paths: Vec<PathBuf> =
                            events.into_iter().map(|e| e.path).collect();
                        paths.sort();
                        paths.dedup();
                        if paths.is_empty() {
                            return;
                        }
                        WatchSignal::Batch(paths)
                    }
                    Err(e) => WatchSignal::Failed(e.to_string()),
                };
                let mut guard = batched.lock();
                (&mut **guard)(signal);
            },
        );
        // Never leave the slot armed for the next construction on this thread.
        RESCAN_SINK.with(|slot| *slot.borrow_mut() = None);

        let mut debouncer = built.map_err(|e| e.to_string())?;
        for spec in specs {
            let mode = if spec.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(e) = debouncer.watcher().watch(&spec.path, mode) {
                if spec.optional && !spec.path.exists() {
                    log::debug!("[repowatch] optional path absent: {}", spec.path.display());
                    continue;
                }
                return Err(format!("watch {} failed: {e}", spec.path.display()));
            }
        }
        Ok(Box::new(NotifyWatch {
            _debouncer: debouncer,
        }))
    }
}
