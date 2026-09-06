//! Background poller for plugin root changes. It hot-reloads modified plugins
//! and registers or deregisters plugin folders without restarting the daemon.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;

use crate::host_traits::{PluginHostCtx, PluginProviderRegistrar, SecretBackendRegistrar};
use crate::provider::AgentProvider;

use super::{load_one, PluginManifest};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct Tracked {
    manifest_path: PathBuf,
    manifest_mtime: SystemTime,
    entry_mtime: SystemTime,
    config_mtime: SystemTime,
}

/// The file state a load was attempted against. A load that cannot succeed is
/// retried only once this changes, so a broken plugin stops instead of spinning.
#[derive(PartialEq)]
struct LoadFingerprint {
    manifest_mtime: SystemTime,
    entry_mtime: SystemTime,
    config_mtime: SystemTime,
}

#[derive(Clone)]
pub struct CapabilityRegistries {
    pub transcribe: Arc<crate::transcribe::TranscribeRegistry>,
    pub glance: crate::glance::GlanceRegistryHandle,
    pub bubble: crate::bubble::BubbleRegistryHandle,
    pub view: crate::view_host::ViewRegistryHandle,
    pub chatbackend: crate::chatbackend_provider::ChatBackendRegistryHandle,
    pub chat_preprocessor: crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
    pub agent_command: crate::agent_commands::AgentCommandRegistryHandle,
    pub secret: Option<Arc<dyn SecretBackendRegistrar>>,
}

impl CapabilityRegistries {
    pub fn register_plugin(&self, manifest: &PluginManifest, vm: &Arc<super::PluginVm>) {
        let id = &manifest.id;
        let caps = &manifest.capabilities;
        // Keep the models only when this manifest still declares the capability:
        // they are about to be replaced. A manifest that drops chatBackend is a
        if caps.chat_backend.enabled() {
            self.deregister_plugin_keeping_models(id);
        } else {
            self.deregister_plugin(id);
        }
        if caps.transcriber {
            self.transcribe.register(
                id.clone(),
                Arc::new(super::JsTranscriber::new(vm.clone(), id.clone())),
            );
        }
        if caps.secret_backend.enabled() {
            if let Some(reg) = &self.secret {
                reg.register_secret_backend(
                    id.clone(),
                    vm.clone(),
                    caps.secret_backend.auto_unlock(),
                );
            }
        }
        if caps.glance_view {
            self.glance.register(
                id.clone(),
                Arc::new(super::JsGlanceProvider::new(vm.clone(), id.clone())),
            );
        }
        if caps.status_bubble {
            self.bubble.register(
                id.clone(),
                Arc::new(super::JsBubbleProvider::new(vm.clone(), id.clone())),
            );
        }
        if caps.view {
            self.view.register(
                id.clone(),
                Arc::new(super::JsViewProvider::new(vm.clone(), id.clone())),
            );
        }
        if caps.chat_backend.enabled() {
            self.chatbackend.register(
                id.clone(),
                Arc::new(super::JsChatBackend::from_manifest(vm.clone(), manifest)),
            );
        }
        if caps.chat_preprocessor.enabled() {
            self.chat_preprocessor.register(
                id.clone(),
                Arc::new(super::JsChatPreprocessor::new(vm.clone(), id.clone())),
            );
        }
        if vm.agent_commands_allowed() {
            self.agent_command.register(
                id.clone(),
                Arc::new(super::JsAgentCommandHandler::new(
                    vm.clone(),
                    id.clone(),
                    true,
                )),
            );
        }
        if caps.transcriber || caps.chat_preprocessor.enabled() {
            warn_stage_conflicts(&self.transcribe, &self.chat_preprocessor);
        }
    }

    pub fn deregister_plugin(&self, id: &str) {
        self.deregister_common(id);
        self.chatbackend.unregister(id);
    }

    /// The reload variant: the plugin is re-registered on the next line, and its
    /// models resolve asynchronously afterwards, so the catalog must be swapped
    /// rather than emptied and refilled.
    fn deregister_plugin_keeping_models(&self, id: &str) {
        self.deregister_common(id);
        self.chatbackend.unregister_for_reload(id);
    }

    fn deregister_common(&self, id: &str) {
        self.transcribe.unregister(id);
        if let Some(reg) = &self.secret {
            reg.unregister_secret_backend(id);
        }
        self.glance.unregister(id);
        self.bubble.unregister(id);
        self.view.unregister(id);
        self.chat_preprocessor.unregister(id);
        self.agent_command.unregister(id);
    }
}

pub struct HotReloadRegistry {
    roots: Vec<PathBuf>,
    registrar: Arc<dyn PluginProviderRegistrar>,
    caps: CapabilityRegistries,
    state: Mutex<HashMap<String, Tracked>>,
    vms: Mutex<HashMap<String, std::sync::Weak<super::PluginVm>>>,
    activated_startup: Mutex<HashSet<String>>,
    pending_reload: Mutex<HashSet<String>>,
    failed: Mutex<HashMap<PathBuf, LoadFingerprint>>,
    service: Mutex<Option<std::sync::Weak<dyn PluginHostCtx>>>,
    #[cfg(test)]
    load_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    failed_attempt_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    contribution_event_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    view_reload_events: Mutex<Vec<String>>,
}

static WATCHER: OnceLock<Arc<HotReloadRegistry>> = OnceLock::new();

pub fn pending_reload_ids() -> Vec<String> {
    WATCHER
        .get()
        .map(|w| w.pending_reload_ids())
        .unwrap_or_default()
}

pub fn set_watcher_service(svc: std::sync::Weak<dyn PluginHostCtx>) {
    if let Some(w) = WATCHER.get() {
        w.set_service_ref(svc);
    }
}

impl HotReloadRegistry {
    pub fn new(
        roots: Vec<PathBuf>,
        registrar: Arc<dyn PluginProviderRegistrar>,
        caps: CapabilityRegistries,
    ) -> Arc<Self> {
        Arc::new(Self {
            roots,
            registrar,
            caps,
            state: Mutex::new(HashMap::new()),
            vms: Mutex::new(HashMap::new()),
            activated_startup: Mutex::new(HashSet::new()),
            pending_reload: Mutex::new(HashSet::new()),
            failed: Mutex::new(HashMap::new()),
            service: Mutex::new(None),
            #[cfg(test)]
            load_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            failed_attempt_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            contribution_event_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            view_reload_events: Mutex::new(Vec::new()),
        })
    }

    pub fn set_service_ref(&self, svc: std::sync::Weak<dyn PluginHostCtx>) {
        *self.service.lock() = Some(svc);
    }

    fn bind_service(&self, vm: &Arc<super::PluginVm>) -> bool {
        let svc = self.service.lock().clone();
        match svc {
            Some(weak) => vm.set_service(weak),
            None => false,
        }
    }

    pub fn register_vm(&self, id: &str, vm: &Arc<super::PluginVm>) {
        self.vms.lock().insert(id.to_string(), Arc::downgrade(vm));
    }

    fn existing_vm(&self, id: &str) -> Option<Arc<super::PluginVm>> {
        self.vms.lock().get(id).and_then(std::sync::Weak::upgrade)
    }

    pub fn seed(&self) {
        let candidates = self.discover_candidates();
        let mut state = self.state.lock();
        let mut activated = self.activated_startup.lock();
        for paths in candidates.values() {
            for manifest_path in paths.iter().rev() {
                let Ok(manifest) = PluginManifest::load(manifest_path) else {
                    continue;
                };
                let Some(entry_path) = manifest.entry_path() else {
                    continue;
                };
                if !entry_path.exists() {
                    continue;
                }
                state.insert(
                    manifest.id.clone(),
                    Tracked {
                        manifest_path: manifest_path.clone(),
                        manifest_mtime: mtime_or_epoch(manifest_path),
                        entry_mtime: mtime_or_epoch(&entry_path),
                        config_mtime: config_mtime_for(manifest_path),
                    },
                );
                activated.insert(manifest.id);
                break;
            }
        }
    }

    pub fn pending_reload_ids(&self) -> Vec<String> {
        self.pending_reload.lock().iter().cloned().collect()
    }

    /// Manifest paths whose last load attempt failed. They stay recorded — and
    /// unattempted — until the files they failed against change.
    pub fn failed_load_paths(&self) -> Vec<PathBuf> {
        self.failed.lock().keys().cloned().collect()
    }

    fn already_failed(&self, path: &std::path::Path, fingerprint: &LoadFingerprint) -> bool {
        self.failed
            .lock()
            .get(path)
            .is_some_and(|f| f == fingerprint)
    }

    /// A recorded failure blocks the retry until the files change; a changed
    /// fingerprint drops the record so the next attempt runs.
    fn retry_allowed(&self, path: &std::path::Path, fingerprint: &LoadFingerprint) -> bool {
        let mut failed = self.failed.lock();
        let Some(recorded) = failed.get(path) else {
            return true;
        };
        if recorded == fingerprint {
            return false;
        }
        failed.remove(path);
        true
    }

    fn record_failure(&self, path: &std::path::Path, fingerprint: LoadFingerprint) {
        self.failed.lock().insert(path.to_path_buf(), fingerprint);
        #[cfg(test)]
        self.failed_attempt_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn capability_registries(&self) -> &CapabilityRegistries {
        &self.caps
    }

    /// An open plugin view keeps running the bundle it already loaded, so a
    /// reload that swaps only the VM leaves the user looking at the old UI.
    fn emit_plugin_view_reloaded(&self, id: &str) {
        #[cfg(test)]
        self.view_reload_events.lock().push(id.to_string());
        let svc = self.service.lock().clone().and_then(|w| w.upgrade());
        if let Some(svc) = svc {
            svc.contributions_changed(&format!("plugin_view:{id}"));
        }
    }

    fn emit_agent_provider_changed(&self) {
        #[cfg(test)]
        self.contribution_event_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let svc = self.service.lock().clone().and_then(|w| w.upgrade());
        if let Some(svc) = svc {
            svc.contributions_changed("agent_provider");
        }
    }

    pub fn start(self: &Arc<Self>) {
        let _ = WATCHER.set(self.clone());
        let me = self.clone();
        std::thread::Builder::new()
            .name("codeterm-plugin-watcher".into())
            .spawn(move || loop {
                me.tick();
                std::thread::sleep(POLL_INTERVAL);
            })
            .ok();
    }

    pub fn tick(&self) {
        let mut seen: Vec<String> = Vec::new();
        let candidates = self.discover_candidates();
        for paths in candidates.values() {
            for manifest_path in paths.iter().rev() {
                if let Some(id) = self.reconcile_one(manifest_path) {
                    seen.push(id);
                    break;
                }
            }
        }
        let removed: Vec<String> = {
            let mut state = self.state.lock();
            let removed: Vec<String> = state
                .keys()
                .filter(|id| !seen.contains(id))
                .cloned()
                .collect();
            for id in &removed {
                state.remove(id);
            }
            removed
        };
        let mut provider_changed = false;
        for id in removed {
            self.vms.lock().remove(&id);
            provider_changed |= self.registrar.unregister_provider(&id);
            provider_changed |= self.caps.chatbackend.contains(&id);
            self.caps.deregister_plugin(&id);
            self.pending_reload.lock().remove(&id);
            self.activated_startup.lock().remove(&id);
            log::info!("[plugin-watcher] unregistered {id} (folder removed)");
        }
        if provider_changed {
            self.emit_agent_provider_changed();
        }
        self.failed.lock().retain(|path, _| path.exists());
    }

    fn discover_candidates(&self) -> BTreeMap<String, Vec<PathBuf>> {
        let mut candidates: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for root in &self.roots {
            if !root.exists() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let manifest_path = path.join("plugin.json");
                if !manifest_path.exists() {
                    continue;
                }
                let manifest = match PluginManifest::load(&manifest_path) {
                    Ok(m) => m,
                    Err(e) => {
                        // The entry is unreachable while the manifest is unparsable, so the
                        // manifest's own mtime is what a fix has to move.
                        let fingerprint = LoadFingerprint {
                            manifest_mtime: mtime_or_epoch(&manifest_path),
                            entry_mtime: SystemTime::UNIX_EPOCH,
                            config_mtime: config_mtime_for(&manifest_path),
                        };
                        if !self.already_failed(&manifest_path, &fingerprint) {
                            log::warn!("[plugin-watcher] {} parse: {e}", manifest_path.display());
                            self.record_failure(&manifest_path, fingerprint);
                        }
                        continue;
                    }
                };
                candidates
                    .entry(manifest.id)
                    .or_default()
                    .push(manifest_path);
            }
        }
        candidates
    }

    fn reconcile_one(&self, manifest_path: &std::path::Path) -> Option<String> {
        let manifest_mtime = mtime_or_epoch(manifest_path);
        let manifest = match PluginManifest::load(manifest_path) {
            Ok(m) => m,
            Err(e) => {
                let fingerprint = LoadFingerprint {
                    manifest_mtime,
                    entry_mtime: SystemTime::UNIX_EPOCH,
                    config_mtime: config_mtime_for(manifest_path),
                };
                if !self.already_failed(manifest_path, &fingerprint) {
                    log::warn!("[plugin-watcher] {} parse: {e}", manifest_path.display());
                    self.record_failure(manifest_path, fingerprint);
                }
                return None;
            }
        };
        let entry_path = manifest.entry_path()?;
        if !entry_path.exists() {
            return None;
        }
        let entry_mtime = mtime_or_epoch(&entry_path);
        let config_mtime = config_mtime_for(manifest_path);

        let fingerprint = LoadFingerprint {
            manifest_mtime,
            entry_mtime,
            config_mtime,
        };
        if !self.retry_allowed(manifest_path, &fingerprint) {
            return None;
        }

        let prior = self.state.lock().get(&manifest.id).cloned();
        if let Some(prev) = prior {
            // A duplicate id can move to a different root's file; mtimes alone can't
            // tell that apart from "unchanged" if they happen to collide (coarse OS clocks).
            let path_changed = prev.manifest_path != manifest_path;
            let manifest_changed = path_changed || prev.manifest_mtime != manifest_mtime;
            let entry_changed = path_changed || prev.entry_mtime != entry_mtime;
            let config_changed = prev.config_mtime != config_mtime;
            if !manifest_changed && !entry_changed && !config_changed {
                return Some(manifest.id);
            }
            if config_changed && !manifest_changed && !entry_changed {
                if let Some(vm) = self.existing_vm(&manifest.id) {
                    let settings = manifest_path
                        .parent()
                        .map(|dir| super::plugin_config::resolve_settings_json(dir, &manifest.id))
                        .unwrap_or_else(|| "null".to_string());
                    vm.reload_settings(settings);
                    if let Some(tracked) = self.state.lock().get_mut(&manifest.id) {
                        tracked.config_mtime = config_mtime;
                    }
                    log::info!(
                        "[plugin-watcher] {} config-only change — re-stamped settings into live VM (no rebuild, sessions preserved)",
                        manifest.id
                    );
                    return Some(manifest.id);
                }
            }
            log::info!(
                "[plugin-watcher] {} changed (manifest={manifest_changed}, entry={entry_changed}, config={config_changed}) — reloading",
                manifest.id
            );
        }
        self.full_load(manifest_path, manifest_mtime, entry_mtime, config_mtime)
    }

    fn full_load(
        &self,
        manifest_path: &std::path::Path,
        manifest_mtime: SystemTime,
        entry_mtime: SystemTime,
        config_mtime: SystemTime,
    ) -> Option<String> {
        match load_one(manifest_path) {
            Ok(mut provider) => {
                let id = provider.manifest().id.clone();
                let version = provider.manifest().version.clone();
                let was_spawnable_chatbackend = self
                    .caps
                    .chatbackend
                    .get(&id)
                    .is_some_and(|backend| backend.spawnable());
                self.register_vm(&id, provider.vm());
                let activated_at_startup = self.activated_startup.lock().contains(&id);
                if provider.manifest().requires_reload && !activated_at_startup {
                    self.pending_reload.lock().insert(id.clone());
                    self.state.lock().insert(
                        id.clone(),
                        Tracked {
                            manifest_path: manifest_path.to_path_buf(),
                            manifest_mtime,
                            entry_mtime,
                            config_mtime,
                        },
                    );
                    log::info!(
                        "[plugin-watcher] {id} declares requiresReload — pending until daemon reload"
                    );
                    return Some(id);
                }
                self.bind_service(provider.vm());
                if let Some(api_port) = runtime_api_port() {
                    provider.ensure_integration_on_startup(api_port);
                }
                self.caps
                    .register_plugin(provider.manifest(), provider.vm());
                if provider.manifest().capabilities.view {
                    self.emit_plugin_view_reloaded(&id);
                }
                if provider.manifest().capabilities.agent_provider {
                    provider.set_registrar(self.registrar.clone());
                    self.registrar.register_provider(Box::new(provider));
                    log::info!("[plugin-watcher] registered {id}");
                    self.emit_agent_provider_changed();
                } else {
                    if was_spawnable_chatbackend
                        || provider.manifest().capabilities.chat_backend_spawnable()
                    {
                        self.emit_agent_provider_changed();
                    }
                    log::info!(
                        "[plugin-watcher] {id} capability registered live; not an agent provider — skipped agent registry"
                    );
                }
                self.state.lock().insert(
                    id.clone(),
                    Tracked {
                        manifest_path: manifest_path.to_path_buf(),
                        manifest_mtime,
                        entry_mtime,
                        config_mtime,
                    },
                );
                crate::audit::append_daemon_reloaded(&id, &version);
                #[cfg(test)]
                self.load_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some(id)
            }
            Err(e) => {
                log::warn!(
                    "[plugin-watcher] {} load failed: {e}",
                    manifest_path.display()
                );
                self.record_failure(
                    manifest_path,
                    LoadFingerprint {
                        manifest_mtime,
                        entry_mtime,
                        config_mtime,
                    },
                );
                None
            }
        }
    }

    pub fn tracked_ids(&self) -> Vec<String> {
        self.state.lock().keys().cloned().collect()
    }

    #[cfg(test)]
    pub fn load_count(&self) -> usize {
        self.load_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn failed_attempt_count(&self) -> usize {
        self.failed_attempt_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn view_reload_events(&self) -> Vec<String> {
        self.view_reload_events.lock().clone()
    }

    #[cfg(test)]
    pub fn contribution_event_count(&self) -> usize {
        self.contribution_event_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn runtime_api_port() -> Option<u16> {
    std::env::var("CODETERM_API_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn mtime_or_epoch(p: &std::path::Path) -> SystemTime {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn config_mtime_for(manifest_path: &std::path::Path) -> SystemTime {
    manifest_path
        .parent()
        .map(|dir| mtime_or_epoch(&super::plugin_config::config_yaml_path(dir)))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn warn_stage_conflicts(
    transcribers: &crate::transcribe::TranscribeRegistry,
    preprocessors: &dyn crate::chatpreprocessor::ChatPreprocessorPort,
) {
    let voice = transcribers.ids();
    let text = preprocessors.preprocessor_ids();
    let mut voice_ids: Vec<String> = Vec::new();
    let mut text_ids: Vec<String> = Vec::new();
    for id in &voice {
        voice_ids.push(id.clone());
    }
    for id in &text {
        text_ids.push(id.clone());
    }
    if voice_ids.len() > 1 {
        log::warn!(
            "[chatpipeline] voice slot grabbed by >1 stage ({}); they would run twice — uninstall one",
            voice_ids.join(", ")
        );
    }
    if text_ids.len() > 1 {
        log::warn!(
            "[chatpipeline] text slot grabbed by >1 stage ({}); they would run twice — uninstall one",
            text_ids.join(", ")
        );
    }
}

#[cfg(test)]
mod tests;
