use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::PathBuf;
use std::time::Duration;

/// Coalescing window for a burst an event already started. This is not a
/// timer: it never *discovers* a change and the absence of events is never
/// read as information — elapsed time only delays delivery of events the OS
/// already produced. 200ms matches the plan watcher's proven window and
/// collapses a build's file churn into one batch.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSpec {
    pub path: PathBuf,
    pub recursive: bool,
    /// `packed-refs` and `refs/` may not exist yet in a fresh repository; a
    /// missing optional path is covered by its parent's non-recursive watch.
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

pub type BatchHandler = Box<dyn FnMut(Vec<PathBuf>) + Send + 'static>;

/// A live OS watch. Dropping it releases every backend handle it owns.
pub trait ActiveWatch: Send {}

/// The construction seam. Production builds a `notify` watcher; a test can
/// inject a backend that refuses, which is the only honest way to exercise
/// permission denial and handle exhaustion.
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
                // A backend hiccup is logged, never converted into a change and
                // never into an unavailable signal the UI would act on.
                Err(e) => {
                    log::warn!("[repowatch] backend error: {e}");
                    return;
                }
            };
            let mut paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
            paths.sort();
            paths.dedup();
            if !paths.is_empty() {
                handler(paths);
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
                    log::debug!(
                        "[repowatch] optional path absent, skipped: {}",
                        spec.path.display()
                    );
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
