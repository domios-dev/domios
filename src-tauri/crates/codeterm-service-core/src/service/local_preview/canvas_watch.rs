//! Change events for open `<ds-canvas src>` files. Each watch observes the file's directory,
//! because an atomic replace swaps the file's identity; at most `MAX_WATCHES` are held at once.

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

static WATCHES: LazyLock<Mutex<Vec<Watch>>> = LazyLock::new(Default::default);

pub fn event_name(tab: &str) -> String {
    format!("local-canvas-changed-{tab}")
}

fn key(requested: &str) -> String {
    path_key(Path::new(requested))
}

/// Emits `local-canvas-changed-{tab}` with the requested path whenever the file changes.
pub(super) fn watch(
    ui: Arc<dyn UiEvents>,
    tab: &str,
    requested: &str,
    canonical: &Path,
) -> Result<(), String> {
    let key = key(requested);
    let mut watches = WATCHES.lock();
    if let Some(existing) = watches
        .iter_mut()
        .find(|watch| watch.tab == tab && watch.key == key)
    {
        existing.holders += 1;
        return Ok(());
    }
    let directory = canonical.parent().ok_or("Canvas file has no directory")?;
    let name = canonical
        .file_name()
        .ok_or("Canvas file has no name")?
        .to_os_string();
    let event = event_name(tab);
    let path = requested.to_owned();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
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
    if watches.len() >= MAX_WATCHES {
        watches.remove(0);
    }
    watches.push(Watch {
        tab: tab.to_owned(),
        key,
        holders: 1,
        _watcher: watcher,
    });
    Ok(())
}

pub(super) fn release(tab: &str, requested: &str) {
    let key = key(requested);
    let mut watches = WATCHES.lock();
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

pub fn drop_tab(tab: &str) {
    WATCHES.lock().retain(|watch| watch.tab != tab);
}
