//! Re-probe a provider's model catalog when its CLI is rewritten.
//!
//! A CLI self-update is a file write, and a file write is an event — so the
//! catalog never has to guess with a timer whether the answer it cached still
//! describes the binary on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::event::{MetadataKind, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;

use codeterm_plugin::chatbackend_provider::ChatBackendProvider;
use codeterm_plugin::provider::AgentProvider;

use crate::agent::model_catalog;

/// Keeps one watcher per provider alive for the daemon's lifetime.
#[derive(Default)]
pub struct ProviderBinaryWatchers {
    watchers: Mutex<HashMap<String, InputWatch>>,
}

type OnChange = Arc<dyn Fn() + Send + Sync>;

struct InputWatch {
    targets: Vec<PathBuf>,
    callback: Arc<Mutex<OnChange>>,
    _watcher: RecommendedWatcher,
}

#[derive(Debug, PartialEq, Eq)]
enum WatchInstall {
    Installed(PathBuf),
    Reused,
    Unavailable,
}

#[derive(PartialEq, Eq)]
struct InputStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

fn input_stamps(targets: &[PathBuf]) -> Vec<Option<InputStamp>> {
    targets
        .iter()
        .map(|path| {
            std::fs::metadata(path).ok().map(|metadata| InputStamp {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })
        })
        .collect()
}

fn changes_input(event: &Event, targets: &[PathBuf]) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && !matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    ) && touches_any(event, targets)
}

impl ProviderBinaryWatchers {
    fn watch(
        &self,
        key: String,
        mut targets: Vec<PathBuf>,
        on_change: Arc<dyn Fn() + Send + Sync>,
    ) -> WatchInstall {
        targets.sort();
        targets.dedup();
        let mut watchers = self.watchers.lock();
        if let Some(existing) = watchers.get(&key).filter(|watch| watch.targets == targets) {
            *existing.callback.lock() = on_change;
            return WatchInstall::Reused;
        }
        let watch_dirs: Vec<PathBuf> = targets
            .iter()
            .filter_map(|target| target.parent().map(Path::to_path_buf))
            .filter(|dir| dir.is_dir())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if watch_dirs.is_empty() {
            watchers.remove(&key);
            return WatchInstall::Unavailable;
        }
        let watched_targets = targets.clone();
        let callback = Arc::new(Mutex::new(on_change));
        let event_callback = callback.clone();
        let role = format!("catalogue-watch:{key}");
        let mut stamps = input_stamps(&targets);
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let _role = codeterm_util::named_thread::Scope::enter(&role);
            let Ok(event) = event else { return };
            if !changes_input(&event, &watched_targets) {
                return;
            }
            let current = input_stamps(&watched_targets);
            if current == stamps {
                return;
            }
            stamps = current;
            let on_change = event_callback.lock().clone();
            on_change();
        });
        let Ok(mut watcher) = watcher else {
            return WatchInstall::Unavailable;
        };
        for dir in watch_dirs {
            if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
                return WatchInstall::Unavailable;
            }
        }
        watchers.insert(
            key,
            InputWatch {
                targets: targets.clone(),
                callback,
                _watcher: watcher,
            },
        );
        WatchInstall::Installed(targets.remove(0))
    }
}

#[cfg(test)]
#[path = "model_binary_watch/tests.rs"]
mod tests;

fn touches_any(event: &Event, targets: &[PathBuf]) -> bool {
    event
        .paths
        .iter()
        .any(|path| targets.iter().any(|target| path == target))
}

/// Re-probe without the TTL after a declared catalogue input changes.
pub fn on_catalogue_input_changed(provider: Arc<dyn AgentProvider>) {
    crate::service::capability_probe::invalidate_and_rebuild(provider.id());
    model_catalog::invalidate(provider.id());
    refresh_agent_provider(provider, true);
}

pub fn refresh_agent_provider(provider: Arc<dyn AgentProvider>, force: bool) {
    install(provider.clone());
    tauri::async_runtime::spawn(async move {
        let _ = if force {
            model_catalog::refresh_after_catalogue_input_change(provider.clone()).await
        } else {
            model_catalog::refresh_models(provider.clone(), false).await
        };
        install(provider);
    });
}

fn watchers() -> &'static ProviderBinaryWatchers {
    static WATCHERS: std::sync::OnceLock<ProviderBinaryWatchers> = std::sync::OnceLock::new();
    WATCHERS.get_or_init(ProviderBinaryWatchers::default)
}

/// Install the watch and the refresh it triggers. A provider whose CLI is not
/// installed is simply not watched; the next forced refresh resolves it.
pub fn install(provider: Arc<dyn AgentProvider>) {
    let mut targets = provider
        .model_catalogue_capability()
        .fingerprint_files(provider.model_credential_files());
    let binary = provider.cli_binary().and_then(|binary| {
        match codeterm_plugin::js_runtime::probe_bin(binary).resolution {
            codeterm_plugin::js_runtime::BinResolution::Found(path) => Some(path),
            _ => None,
        }
    });
    if let Some(binary) = &binary {
        targets.push(binary.clone());
    }
    let watched_provider = provider.clone();
    let key = format!("pty:{}", provider.id());
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        on_catalogue_input_changed(watched_provider.clone());
    });
    match watchers().watch(key, targets, on_change) {
        WatchInstall::Installed(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        WatchInstall::Unavailable => {
            log::debug!("[model_catalog] no declared catalogue inputs to watch")
        }
        WatchInstall::Reused => {}
    }
}

pub fn install_chatbackend(
    provider: Arc<dyn ChatBackendProvider>,
    registry: crate::chatbackend::ChatBackendRegistryHandle,
) {
    let id = provider.id().to_string();
    let targets = provider.model_catalogue_input_files();
    let refresh_registry = registry.clone();
    let refresh_id = id.clone();
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        model_catalog::invalidate(&refresh_id);
        let _ = refresh_registry.refresh_provider(&refresh_id);
    });
    match watchers().watch(format!("chatbackend:{id}"), targets, on_change) {
        WatchInstall::Installed(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        WatchInstall::Unavailable => {
            log::debug!("[model_catalog] no declared chatBackend catalogue inputs to watch")
        }
        WatchInstall::Reused => {}
    }
}

pub fn refresh_after_settings_change(
    agents: &crate::agent::registry::AgentRegistry,
    chat_backends: &crate::chatbackend::ChatBackendRegistry,
    provider_ids: impl IntoIterator<Item = String>,
) {
    for provider_id in provider_ids {
        if chat_backends.contains(&provider_id) {
            crate::service::capability_probe::invalidate_and_rebuild(&provider_id);
            chat_backends.refresh_after_input_change(&provider_id);
        } else {
            refresh_agent_after_settings_change(agents, &provider_id);
        }
    }
}

pub fn refresh_agent_after_settings_change(
    agents: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) {
    crate::service::capability_probe::invalidate_and_rebuild(provider_id);
    if let Some(provider) = agents.provider_arc(provider_id) {
        model_catalog::invalidate(provider_id);
        refresh_agent_provider(provider, true);
    }
}
