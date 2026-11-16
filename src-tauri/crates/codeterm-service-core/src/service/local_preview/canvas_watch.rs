//! Change events for open `<ds-canvas src>` files. Each watch observes the file's directory,
//! because an atomic replace swaps the file's identity. At most `MAX_WATCHES` are held at once;
//! a further canvas is refused rather than taking a live viewer's watch.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;

use super::references::path_key;
use crate::ports::host::UiEvents;

const MAX_WATCHES: usize = 16;

struct Watch {
    tab: String,
    key: String,
    holders: usize,
    _watcher: RecommendedWatcher,
}

pub(super) struct CanvasWatches {
    entries: Mutex<Vec<Watch>>,
    capacity: usize,
}

static WATCHES: LazyLock<CanvasWatches> = LazyLock::new(|| CanvasWatches::new(MAX_WATCHES));

pub(super) fn watches() -> &'static CanvasWatches {
    &WATCHES
}

pub fn event_name(tab: &str) -> String {
    format!("local-canvas-changed-{tab}")
}

fn key(requested: &str) -> String {
    path_key(Path::new(requested))
}

impl CanvasWatches {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::default(),
            capacity,
        }
    }

    /// Emits `local-canvas-changed-{tab}` with the requested path whenever the file changes.
    /// `Ok(false)` means every slot is held by an open viewer, so this one shows no live updates.
    pub(super) fn watch(
        &self,
        ui: Arc<dyn UiEvents>,
        tab: &str,
        requested: &str,
        canonical: &Path,
    ) -> Result<bool, String> {
        let key = key(requested);
        let mut watches = self.entries.lock();
        if let Some(existing) = watches
            .iter_mut()
            .find(|watch| watch.tab == tab && watch.key == key)
        {
            existing.holders += 1;
            return Ok(true);
        }
        if watches.len() >= self.capacity {
            return Ok(false);
        }
        let directory = canonical.parent().ok_or("Canvas file has no directory")?;
        let name = canonical
            .file_name()
            .ok_or("Canvas file has no name")?
            .to_os_string();
        let event = event_name(tab);
        let path = requested.to_owned();
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let Ok(change) = result else {
                    return;
                };
                if matches!(change.kind, EventKind::Access(_)) {
                    return;
                }
                let ours = change.paths.iter().any(|changed| {
                    changed
                        .file_name()
                        .is_some_and(|changed| changed.eq_ignore_ascii_case(&name))
                });
                if ours {
                    ui.emit(&event, serde_json::json!({ "path": path }));
                }
            })
            .map_err(|error| format!("Unable to watch canvas: {error}"))?;
        watcher
            .watch(directory, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Unable to watch canvas: {error}"))?;
        watches.push(Watch {
            tab: tab.to_owned(),
            key,
            holders: 1,
            _watcher: watcher,
        });
        Ok(true)
    }

    pub(super) fn release(&self, tab: &str, requested: &str) {
        let key = key(requested);
        let mut watches = self.entries.lock();
        if let Some(index) = watches
            .iter()
            .position(|watch| watch.tab == tab && watch.key == key)
        {
            watches[index].holders -= 1;
            if watches[index].holders == 0 {
                watches.remove(index);
            }
        }
    }

    fn drop_tab(&self, tab: &str) {
        self.entries.lock().retain(|watch| watch.tab != tab);
    }
}

pub fn drop_tab(tab: &str) {
    WATCHES.drop_tab(tab);
}

#[cfg(test)]
mod tests;
