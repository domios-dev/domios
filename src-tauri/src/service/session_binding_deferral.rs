use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeferredSessionBinding {
    pub generation: u64,
    pub tab_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub previous_session_id: String,
    pub transcript_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RefusedSessionBinding {
    pub session_id: String,
    pub previous_session_id: Option<String>,
    pub reason: String,
}

struct WatchedBinding {
    binding: DeferredSessionBinding,
    _watcher: RecommendedWatcher,
}

#[derive(Default)]
pub(crate) struct DeferredSessionBindingRegistry {
    next_generation: AtomicU64,
    entries: Mutex<HashMap<String, WatchedBinding>>,
    refusals: Mutex<HashMap<String, RefusedSessionBinding>>,
}

impl DeferredSessionBindingRegistry {
    pub(crate) fn defer(
        &self,
        tab_id: &str,
        provider_id: &str,
        session_id: &str,
        previous_session_id: &str,
        transcript_path: PathBuf,
        on_materialized: Arc<dyn Fn(String, u64) + Send + Sync>,
    ) -> Result<DeferredSessionBinding, String> {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let binding = DeferredSessionBinding {
            generation,
            tab_id: tab_id.to_string(),
            provider_id: provider_id.to_string(),
            session_id: session_id.to_string(),
            previous_session_id: previous_session_id.to_string(),
            transcript_path: transcript_path.clone(),
        };
        let watch_root = nearest_existing_ancestor(&transcript_path).ok_or_else(|| {
            format!(
                "no existing directory can signal transcript materialization at {}",
                transcript_path.display()
            )
        })?;
        let callback_tab = tab_id.to_string();
        let callback_path = transcript_path.clone();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                return;
            }
            if callback_path.is_file() {
                on_materialized(callback_tab.clone(), generation);
            }
        })
        .map_err(|error| format!("could not create transcript watcher: {error}"))?;
        watcher
            .watch(&watch_root, RecursiveMode::Recursive)
            .map_err(|error| format!("could not watch {}: {error}", watch_root.display()))?;
        self.entries.lock().insert(
            tab_id.to_string(),
            WatchedBinding {
                binding: binding.clone(),
                _watcher: watcher,
            },
        );
        self.refusals.lock().remove(tab_id);
        Ok(binding)
    }

    pub(crate) fn get(&self, tab_id: &str, generation: u64) -> Option<DeferredSessionBinding> {
        self.entries
            .lock()
            .get(tab_id)
            .filter(|entry| entry.binding.generation == generation)
            .map(|entry| entry.binding.clone())
    }

    pub(crate) fn clear(&self, tab_id: &str) {
        self.entries.lock().remove(tab_id);
        self.refusals.lock().remove(tab_id);
    }

    pub(crate) fn clear_session(&self, tab_id: &str, session_id: &str) {
        let mut entries = self.entries.lock();
        if entries
            .get(tab_id)
            .is_some_and(|entry| entry.binding.session_id == session_id)
        {
            entries.remove(tab_id);
        }
        let mut refusals = self.refusals.lock();
        if refusals
            .get(tab_id)
            .is_some_and(|refusal| refusal.session_id == session_id)
        {
            refusals.remove(tab_id);
        }
    }

    pub(crate) fn clear_generation(&self, tab_id: &str, generation: u64) {
        let mut entries = self.entries.lock();
        if entries
            .get(tab_id)
            .is_some_and(|entry| entry.binding.generation == generation)
        {
            entries.remove(tab_id);
        }
    }

    /// Retire a deferral the tab has already moved past: it can never bind, and
    /// a record left behind reads as live to everything that looks at it later.
    pub(crate) fn retire_resolved(&self, tab_id: &str, current_session_id: Option<&str>) -> bool {
        let mut entries = self.entries.lock();
        let resolved = entries.get(tab_id).is_some_and(|entry| {
            Some(entry.binding.previous_session_id.as_str()) != current_session_id
        });
        if resolved {
            entries.remove(tab_id);
        }
        resolved
    }

    pub(crate) fn pending(&self, tab_id: &str) -> Option<DeferredSessionBinding> {
        self.entries
            .lock()
            .get(tab_id)
            .map(|entry| entry.binding.clone())
    }

    pub(crate) fn refuse(
        &self,
        tab_id: &str,
        session_id: &str,
        previous_session_id: Option<&str>,
        reason: &str,
    ) {
        self.entries.lock().remove(tab_id);
        self.refusals.lock().insert(
            tab_id.to_string(),
            RefusedSessionBinding {
                session_id: session_id.to_string(),
                previous_session_id: previous_session_id.map(str::to_string),
                reason: reason.to_string(),
            },
        );
    }

    pub(crate) fn refused(&self, tab_id: &str) -> Option<RefusedSessionBinding> {
        self.refusals.lock().get(tab_id).cloned()
    }
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = path.parent();
    while let Some(current) = candidate {
        if current.is_dir() {
            return Some(current.to_path_buf());
        }
        candidate = current.parent();
    }
    None
}
