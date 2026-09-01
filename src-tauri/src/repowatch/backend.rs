use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::PathBuf;
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

/// What a live watch reports. `Failed` is the runtime backend error: the watch
/// can no longer be trusted, so it must reach the UI, not just the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSignal {
    Batch(Vec<PathBuf>),
    Failed(String),
}

pub type BatchHandler = Box<dyn FnMut(WatchSignal) + Send + 'static>;

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

pub struct NotifyBackend;

struct NotifyWatch {
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl ActiveWatch for NotifyWatch {}

impl WatchBackend for NotifyBackend {
    fn build(
        &self,
        specs: &[WatchSpec],
        mut handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
            let events = match result {
                Ok(events) => events,
                Err(e) => {
                    handler(WatchSignal::Failed(e.to_string()));
                    return;
                }
            };
            let mut paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
            paths.sort();
            paths.dedup();
            if !paths.is_empty() {
                handler(WatchSignal::Batch(paths));
            }
        })
        .map_err(|e| e.to_string())?;

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
