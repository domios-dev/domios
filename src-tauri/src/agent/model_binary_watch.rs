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

use codeterm_plugin::chatbackend_provider::ChatBackendProvider;
use codeterm_plugin::provider::AgentProvider;

use crate::agent::model_catalog;

/// Keeps one watcher per provider alive for the daemon's lifetime.
#[derive(Default)]
pub struct ProviderBinaryWatchers {
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
}

impl ProviderBinaryWatchers {
    fn watch(
        &self,
        key: String,
        mut targets: Vec<PathBuf>,
        on_change: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<PathBuf> {
        targets.sort();
        targets.dedup();
        let watch_dirs: Vec<PathBuf> = targets
            .iter()
            .filter_map(|target| target.parent().map(Path::to_path_buf))
            .filter(|dir| dir.is_dir())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if watch_dirs.is_empty() {
            return None;
        }
        let watched_targets = targets.clone();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            if !touches_any(&event, &watched_targets) {
                return;
            }
            on_change();
        })
        .ok()?;
        for dir in watch_dirs {
            watcher.watch(&dir, RecursiveMode::NonRecursive).ok()?;
        }
        self.watchers.lock().insert(key, watcher);
        targets.into_iter().next()
    }
}

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

/// Install the watch and the refresh it triggers. A provider whose CLI has not
/// resolved yet is simply not watched; the next forced refresh resolves it.
pub fn install(provider: Arc<dyn AgentProvider>) {
    let mut targets = provider
        .model_catalogue_capability()
        .fingerprint_files(provider.model_credential_files());
    let binary = provider
        .cli_binary()
        .and_then(codeterm_plugin::js_runtime::resolved_bin_path);
    if let Some(binary) = &binary {
        targets.push(binary.clone());
    }
    let watched_provider = provider.clone();
    let key = format!("pty:{}", provider.id());
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        on_catalogue_input_changed(watched_provider.clone());
    });
    match watchers().watch(key, targets, on_change) {
        Some(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        None => log::debug!("[model_catalog] no declared catalogue inputs to watch"),
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
        Some(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        None => log::debug!("[model_catalog] no declared chatBackend catalogue inputs to watch"),
    }
}

pub fn refresh_after_settings_change(
    agents: &crate::agent::registry::AgentRegistry,
    chat_backends: &crate::chatbackend::ChatBackendRegistry,
    provider_ids: impl IntoIterator<Item = String>,
) {
    for provider_id in provider_ids {
        crate::service::capability_probe::invalidate_and_rebuild(&provider_id);
        if chat_backends.get(&provider_id).is_some() {
            model_catalog::invalidate(&provider_id);
            let _ = chat_backends.refresh_provider(&provider_id);
        } else if let Some(provider) = agents.provider_arc(&provider_id) {
            model_catalog::invalidate(&provider_id);
            refresh_agent_provider(provider, true);
        }
    }
}
