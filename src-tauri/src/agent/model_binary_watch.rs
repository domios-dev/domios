//! Re-probe a provider's model catalog when its CLI is rewritten.
//!
//! A CLI self-update is a file write, and a file write is an event — so the
//! catalog never has to guess with a timer whether the answer it cached still
//! describes the binary on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;

use codeterm_plugin::provider::AgentProvider;

use crate::agent::model_catalog;

/// Keeps one watcher per provider alive for the daemon's lifetime; dropping the
/// registry stops every watch.
#[derive(Default)]
pub struct ProviderBinaryWatchers {
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
}

impl ProviderBinaryWatchers {
    /// Watch the path the resolver actually resolved to — never a guessed one.
    /// A provider whose CLI never resolved has nothing to watch yet; the next
    /// forced refresh re-resolves and this can be called again.
    pub fn watch(
        &self,
        provider: Arc<dyn AgentProvider>,
        on_change: Arc<dyn Fn(Arc<dyn AgentProvider>) + Send + Sync>,
    ) -> Option<PathBuf> {
        let bin = provider.cli_binary()?.to_string();
        let resolved = codeterm_plugin::js_runtime::resolved_bin_path(&bin)?;
        // A self-update replaces the file rather than editing it, so the
        // directory entry is what carries the event on every platform.
        let dir = resolved.parent()?.to_path_buf();
        let provider_id = provider.id().to_string();
        let target = resolved.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| {
                let Ok(event) = event else { return };
                if !matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    return;
                }
                if !touches(&event, &target) {
                    return;
                }
                on_change(provider.clone());
            })
            .ok()?;
        watcher.watch(&dir, RecursiveMode::NonRecursive).ok()?;
        self.watchers.lock().insert(provider_id, watcher);
        Some(resolved)
    }
}

fn touches(event: &Event, target: &Path) -> bool {
    event.paths.iter().any(|path| path == target)
}

/// The change handler the daemon installs: drop the resolver memo, re-probe
/// without the TTL, and tell every surface the answer moved.
pub fn on_binary_changed<F>(provider: Arc<dyn AgentProvider>, notify_updated: F)
where
    F: Fn(String) + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let provider_id = provider.id().to_string();
        if model_catalog::refresh_after_binary_change(provider)
            .await
            .is_some()
        {
            notify_updated(provider_id);
        }
    });
}

fn watchers() -> &'static ProviderBinaryWatchers {
    static WATCHERS: std::sync::OnceLock<ProviderBinaryWatchers> = std::sync::OnceLock::new();
    WATCHERS.get_or_init(ProviderBinaryWatchers::default)
}

/// Install the watch and the refresh it triggers. A provider whose CLI has not
/// resolved yet is simply not watched; the next forced refresh resolves it.
pub fn install(provider: Arc<dyn AgentProvider>, app: tauri::AppHandle) {
    let on_change: Arc<dyn Fn(Arc<dyn AgentProvider>) + Send + Sync> =
        Arc::new(move |changed: Arc<dyn AgentProvider>| {
            let app = app.clone();
            on_binary_changed(changed, move |provider_id| {
                crate::api::events::emit_everywhere(
                    &app,
                    "models-updated",
                    serde_json::json!({ "provider": provider_id }),
                );
            });
        });
    match watchers().watch(provider, on_change) {
        Some(path) => log::info!(
            "[model_catalog] watching {} for provider-binary changes",
            path.display()
        ),
        None => log::debug!("[model_catalog] no resolved provider binary to watch yet"),
    }
}
