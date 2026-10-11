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
    grants_mtime: SystemTime,
}

/// The file state a load was attempted against. A load that cannot succeed is
/// retried only once this changes, so a broken plugin stops instead of spinning.
#[derive(PartialEq)]
struct LoadFingerprint {
    manifest_mtime: SystemTime,
    entry_mtime: SystemTime,
    config_mtime: SystemTime,
}

/// The last failed load attempt for a plugin, surfaced to Settings > Plugins.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginLoadError {
    pub id: String,
    pub path: PathBuf,
    pub message: String,
    pub at_ms: u64,
    pub kept_previous: bool,
}

#[derive(Clone)]
pub struct CapabilityRegistries {
    pub decision_model:
        Arc<crate::capability::CapabilityRegistry<dyn codeterm_types::decision::DecisionModel>>,
    pub transcribe: Arc<crate::transcribe::TranscribeRegistry>,
    pub converter: Arc<crate::converter::ConverterRegistry>,
    pub graph_nodes: Arc<crate::graph_nodes::GraphNodeRegistry>,
    pub event_source: Arc<crate::event_source::EventSourceRegistry>,
    pub glance: crate::glance::GlanceRegistryHandle,
    pub bubble: crate::bubble::BubbleRegistryHandle,
    pub view: crate::view_host::ViewRegistryHandle,
    pub chatbackend: crate::chatbackend_provider::ChatBackendRegistryHandle,
    pub chat_preprocessor: crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
    pub agent_command: crate::agent_commands::AgentCommandRegistryHandle,
    pub webhook_receiver: crate::webhook::WebhookRegistryHandle,
    pub local_recording: crate::local_recording::LocalRecordingRegistryHandle,
    pub tab_menu: crate::tab_menu::TabMenuRegistryHandle,
    pub surfaces: crate::surfaces::SurfaceRegistryHandle,
    pub secret: Option<Arc<dyn SecretBackendRegistrar>>,
}

impl CapabilityRegistries {
    pub fn register_plugin(&self, manifest: &PluginManifest, vm: &Arc<super::PluginVm>) {
        self.register_plugin_with_core(manifest, vm, false);
    }

    pub fn register_plugin_with_core(
        &self,
        manifest: &PluginManifest,
        vm: &Arc<super::PluginVm>,
        core: bool,
    ) {
        let id = &manifest.id;
        let caps = &manifest.capabilities;
        // Models must be removed when a manifest drops `chatBackend`, because they belong to that capability.
        if caps.chat_backend.enabled() {
            self.deregister_plugin_keeping_models(id);
        } else {
            self.deregister_plugin(id);
        }
        if caps.transcriber {
            let transcriber = Arc::new(super::JsTranscriber::new(vm.clone(), id.clone()));
            self.transcribe.register(id.clone(), transcriber.clone());
            if caps.graph_nodes.converters.is_empty() {
                // A transcriber is the audio -> text edge unless the plugin
                // declares its own converters, which win.
                self.converter.register(
                    id.clone(),
                    Arc::new(crate::converter::TranscriberConverter::new(transcriber)),
                );
            }
        }
        if !caps.graph_nodes.converters.is_empty() {
            self.converter.register(
                id.clone(),
                Arc::new(super::JsConverter::new(
                    vm.clone(),
                    caps.graph_nodes.converters.clone(),
                )),
            );
        }
        if !caps.graph_nodes.event_sources.is_empty() && vm.graph_events_allowed() {
            if let Err(error) = self.event_source.register(
                id.clone(),
                Arc::new(super::JsEventSource::new(
                    vm.clone(),
                    caps.graph_nodes.event_sources.clone(),
                )),
            ) {
                log::error!("[plugin] event source registration refused: {error}");
            }
            if let Err(error) = self.graph_nodes.register(id.clone(), &caps.graph_nodes) {
                log::error!("[plugin] graph node catalog registration refused: {error}");
            }
        }
        if caps.decision_model.enabled {
            self.decision_model.register(
                id.clone(),
                Arc::new(super::JsDecisionModel::new(
                    vm.clone(),
                    id.clone(),
                    caps.decision_model.token_mode,
                )),
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
                Arc::new(super::JsViewProvider::with_webhook_receiver(
                    vm.clone(),
                    id.clone(),
                    caps.webhook_receiver.as_ref().map(|receiver| receiver.kind),
                )),
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
        if let Some(receiver) = &caps.webhook_receiver {
            self.webhook_receiver.register(
                id.clone(),
                Arc::new(
                    super::JsWebhookReceiver::with_icon(
                        vm.clone(),
                        id.clone(),
                        receiver.kind,
                        manifest.icon_svg.clone(),
                    )
                    .accepting_audio(receiver.audio),
                ),
            );
        }
        if let Some(trigger) = &caps.local_recording_trigger {
            self.local_recording
                .register(id, trigger.clone(), vm.local_recording_allowed());
        }
        if !caps.tab_menu.is_empty() {
            self.tab_menu
                .register(id.clone(), Arc::new(caps.tab_menu.clone()));
        }
        let renderable = renderable_surfaces(caps);
        if !renderable.is_empty() {
            self.surfaces.register(id.clone(), Arc::new(renderable));
        }
        if vm.agent_commands_allowed() {
            self.agent_command.register(
                id.clone(),
                Arc::new(
                    super::JsAgentCommandHandler::new(vm.clone(), id.clone(), true)
                        .with_catalog(super::catalog_from_manifest(manifest, core)),
                ),
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
        self.decision_model.unregister(id);
        self.transcribe.unregister(id);
        self.converter.unregister(id);
        self.graph_nodes.unregister(id);
        self.event_source.unregister(id);
        if let Some(reg) = &self.secret {
            reg.unregister_secret_backend(id);
        }
        self.glance.unregister(id);
        self.bubble.unregister(id);
        self.view.unregister(id);
        self.chat_preprocessor.unregister(id);
        self.agent_command.unregister(id);
        self.webhook_receiver.unregister(id);
        self.local_recording.unregister(id);
        self.tab_menu.unregister(id);
        self.surfaces.unregister(id);
    }
}

/// A view surface needs the plugin's view; a stream surface renders from a core runtime instead.
fn renderable_surfaces(caps: &super::manifest::Capabilities) -> Vec<crate::surfaces::SurfaceDecl> {
    caps.surfaces
        .iter()
        .filter(|surface| caps.view || surface.stream_runtime().is_some())
        .cloned()
        .collect()
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
    load_errors: Mutex<HashMap<String, PluginLoadError>>,
    service: Mutex<Option<std::sync::Weak<dyn PluginHostCtx>>>,
    reconciliation: Mutex<()>,
    settings_changed: Mutex<Vec<String>>,
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

pub fn refresh_installed_plugins(ids: &[String]) -> Result<(), String> {
    WATCHER
        .get()
        .ok_or("Plugin watcher is not ready")?
        .refresh_installed(ids)
}

pub fn pending_reload_ids() -> Vec<String> {
    WATCHER
        .get()
        .map(|w| w.pending_reload_ids())
        .unwrap_or_default()
}

pub fn plugin_load_errors() -> Vec<PluginLoadError> {
    WATCHER.get().map(|w| w.load_errors()).unwrap_or_default()
}

pub fn set_watcher_service(svc: std::sync::Weak<dyn PluginHostCtx>) {
    if let Some(w) = WATCHER.get() {
        w.set_service_ref(svc);
    }
}

impl HotReloadRegistry {
    fn notify_event_sources_changed(&self) {
        if let Some(service) = self
            .service
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            service.event_sources_changed();
        }
    }

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
            load_errors: Mutex::new(HashMap::new()),
            service: Mutex::new(None),
            reconciliation: Mutex::new(()),
            settings_changed: Mutex::new(Vec::new()),
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
                        config_mtime: config_mtime_for(manifest_path, &manifest.id),
                        grants_mtime: grants_mtime_for(manifest_path),
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

    pub fn load_errors(&self) -> Vec<PluginLoadError> {
        self.load_errors.lock().values().cloned().collect()
    }

    fn tracked_id_at(&self, path: &std::path::Path) -> Option<String> {
        self.state
            .lock()
            .iter()
            .find(|(_, tracked)| tracked.manifest_path == path)
            .map(|(id, _)| id.clone())
    }

    /// Registered plugins whose manifest is still on disk but whose last load attempt
    /// failed; they keep their previous registration until a load succeeds.
    fn held_registrations(&self) -> HashMap<String, PathBuf> {
        let failed = self.failed.lock();
        self.state
            .lock()
            .iter()
            .filter(|(_, t)| failed.contains_key(&t.manifest_path) && t.manifest_path.exists())
            .map(|(id, t)| (id.clone(), t.manifest_path.clone()))
            .collect()
    }

    fn record_failure(
        &self,
        path: &std::path::Path,
        fingerprint: LoadFingerprint,
        stage: &str,
        error: String,
    ) {
        let registered = self.tracked_id_at(path);
        match &registered {
            Some(id) => log::warn!(
                "[plugin-watcher] {} {stage}: {error} — keeping previous registration of {id}",
                path.display()
            ),
            None => log::warn!("[plugin-watcher] {} {stage}: {error}", path.display()),
        }
        let kept_previous = registered.is_some();
        let id = registered.unwrap_or_else(|| error_id_for(path));
        let at_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.load_errors.lock().insert(
            id.clone(),
            PluginLoadError {
                id,
                path: path.to_path_buf(),
                message: format!("{stage}: {error}"),
                at_ms,
                kept_previous,
            },
        );
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

    fn emit_tab_menu_changed(&self) {
        let svc = self.service.lock().clone().and_then(|w| w.upgrade());
        if let Some(svc) = svc {
            svc.contributions_changed(crate::tab_menu::TAB_MENU_CONTRIBUTION_KIND);
        }
    }

    fn emit_agent_commands_changed(&self) {
        let svc = self.service.lock().clone().and_then(|w| w.upgrade());
        if let Some(svc) = svc {
            svc.contributions_changed(crate::agent_verbs::AGENT_COMMANDS_CONTRIBUTION_KIND);
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
        {
            let _reconciliation = self.reconciliation.lock();
            self.tick_inner();
        }
        self.flush_settings_changed();
    }

    pub fn refresh_installed(&self, ids: &[String]) -> Result<(), String> {
        let result = {
            let _reconciliation = self.reconciliation.lock();
            self.refresh_installed_reconciled(ids)
        };
        self.flush_settings_changed();
        result
    }

    /// Settings are already live in the VM; refresh what the host derived from them,
    /// outside the reconciliation lock because the host side takes its own locks.
    fn flush_settings_changed(&self) {
        let ids = std::mem::take(&mut *self.settings_changed.lock());
        for id in ids {
            self.registrar.provider_settings_changed(&id);
            self.caps.chatbackend.refresh_after_input_change(&id);
        }
    }

    fn refresh_installed_reconciled(&self, ids: &[String]) -> Result<(), String> {
        for id in ids {
            self.state.lock().remove(id);
        }
        for (id, paths) in self.discover_candidates() {
            if ids.contains(&id) {
                for path in paths {
                    self.failed.lock().remove(&path);
                }
            }
        }
        self.tick_inner();
        for id in ids {
            if self.pending_reload.lock().contains(id) {
                return Err(format!(
                    "Plugin {id} was installed on disk but requires a daemon reload to activate"
                ));
            }
        }
        let state = self.state.lock();
        for id in ids {
            if !state.contains_key(id) {
                return Err(format!("Plugin {id} was installed on disk but failed live registration; inspect plugin diagnostics"));
            }
        }
        Ok(())
    }

    fn tick_inner(&self) {
        let mut seen: Vec<String> = Vec::new();
        let candidates = self.discover_candidates();
        let held = self.held_registrations();
        for (id, paths) in &candidates {
            // A held plugin retries only its own file, so a lower-priority duplicate cannot replace it.
            let pinned = held.get(id);
            for manifest_path in paths
                .iter()
                .rev()
                .filter(|path| pinned.is_none_or(|pin| pin == *path))
            {
                if let Some(id) = self.reconcile_one(manifest_path) {
                    seen.push(id);
                    break;
                }
            }
        }
        seen.extend(self.held_registrations().into_keys());
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
        let mut tab_menu_changed = false;
        let mut agent_commands_changed = false;
        let event_sources_changed = !removed.is_empty();
        for id in removed {
            self.vms.lock().remove(&id);
            provider_changed |= self.registrar.unregister_provider(&id);
            provider_changed |= self.caps.chatbackend.contains(&id);
            tab_menu_changed |= self.caps.tab_menu.contains(&id);
            agent_commands_changed |= self.caps.agent_command.contains(&id);
            self.caps.deregister_plugin(&id);
            self.pending_reload.lock().remove(&id);
            self.activated_startup.lock().remove(&id);
            self.load_errors.lock().remove(&id);
            log::info!("[plugin-watcher] unregistered {id} (folder removed)");
        }
        if provider_changed {
            self.emit_agent_provider_changed();
        }
        if tab_menu_changed {
            self.emit_tab_menu_changed();
        }
        if agent_commands_changed {
            self.emit_agent_commands_changed();
        }
        if event_sources_changed {
            self.notify_event_sources_changed();
        }
        self.failed.lock().retain(|path, _| path.exists());
        self.load_errors
            .lock()
            .retain(|_, error| error.path.exists());
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
                            config_mtime: SystemTime::UNIX_EPOCH,
                        };
                        if !self.already_failed(&manifest_path, &fingerprint) {
                            self.record_failure(
                                &manifest_path,
                                fingerprint,
                                "parse",
                                e.to_string(),
                            );
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
                    config_mtime: SystemTime::UNIX_EPOCH,
                };
                if !self.already_failed(manifest_path, &fingerprint) {
                    self.record_failure(manifest_path, fingerprint, "parse", e.to_string());
                }
                return None;
            }
        };
        let entry_path = manifest.entry_path()?;
        if !entry_path.exists() {
            return None;
        }
        let entry_mtime = mtime_or_epoch(&entry_path);
        let config_mtime = config_mtime_for(manifest_path, &manifest.id);

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
            let grants_changed = prev.grants_mtime != grants_mtime_for(manifest_path);
            if grants_changed {
                self.caps.local_recording.unregister(&manifest.id);
            }
            if !manifest_changed && !entry_changed && !config_changed && !grants_changed {
                return Some(manifest.id);
            }
            if config_changed && !manifest_changed && !entry_changed && !grants_changed {
                if let Some(vm) = self.existing_vm(&manifest.id) {
                    let settings = manifest_path
                        .parent()
                        .map(|dir| super::plugin_config::resolve_settings_json(dir, &manifest.id))
                        .unwrap_or_else(|| "null".to_string());
                    vm.reload_settings(settings);
                    if let Some(tracked) = self.state.lock().get_mut(&manifest.id) {
                        tracked.config_mtime = config_mtime;
                    }
                    self.settings_changed.lock().push(manifest.id.clone());
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
                self.load_errors
                    .lock()
                    .retain(|key, error| key != &id && error.path != manifest_path);
                let had_chatbackend = self.caps.chatbackend.contains(&id);
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
                            grants_mtime: grants_mtime_for(manifest_path),
                        },
                    );
                    log::info!(
                        "[plugin-watcher] {id} declares requiresReload — pending until daemon reload"
                    );
                    return Some(id);
                }
                self.bind_service(provider.vm());
                if let Some(api_port) = runtime_api_port()
                    .filter(|_| crate::global_writes::machine_global_write_allowed())
                {
                    provider.ensure_integration_on_startup(api_port);
                }
                let had_tab_menu = self.caps.tab_menu.contains(&id);
                let had_agent_commands = self.caps.agent_command.contains(&id);
                let core = super::loader::is_core_plugin(provider.manifest(), provider.trust());
                self.caps
                    .register_plugin_with_core(provider.manifest(), provider.vm(), core);
                self.notify_event_sources_changed();
                if had_tab_menu || self.caps.tab_menu.contains(&id) {
                    self.emit_tab_menu_changed();
                }
                if had_agent_commands || self.caps.agent_command.contains(&id) {
                    self.emit_agent_commands_changed();
                }
                if provider.manifest().capabilities.view {
                    self.emit_plugin_view_reloaded(&id);
                }
                if provider.manifest().capabilities.agent_provider {
                    provider.set_registrar(self.registrar.clone());
                    self.registrar.register_provider(Box::new(provider));
                    log::info!("[plugin-watcher] registered {id}");
                    self.registrar.provider_reloaded(&id);
                    self.emit_agent_provider_changed();
                } else {
                    if had_chatbackend || provider.manifest().capabilities.chat_backend.enabled() {
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
                        grants_mtime: grants_mtime_for(manifest_path),
                    },
                );
                crate::audit::append_daemon_reloaded(&id, &version);
                #[cfg(test)]
                self.load_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some(id)
            }
            Err(e) => {
                self.record_failure(
                    manifest_path,
                    LoadFingerprint {
                        manifest_mtime,
                        entry_mtime,
                        config_mtime,
                    },
                    "load failed",
                    e.to_string(),
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

#[cfg(test)]
mod install_refresh_tests;
#[cfg(test)]
mod settings_change_tests;

fn runtime_api_port() -> Option<u16> {
    std::env::var("CODETERM_API_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
}

/// An unparsable manifest has no trusted id; the raw `id` field, then the folder name, keys its error.
fn error_id_for(manifest_path: &std::path::Path) -> String {
    std::fs::read_to_string(manifest_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("id")?.as_str().map(String::from))
        .or_else(|| {
            manifest_path
                .parent()
                .and_then(|dir| dir.file_name())
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| manifest_path.display().to_string())
}

fn mtime_or_epoch(p: &std::path::Path) -> SystemTime {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn config_mtime_for(manifest_path: &std::path::Path, plugin_id: &str) -> SystemTime {
    manifest_path
        .parent()
        .map(|dir| mtime_or_epoch(&super::plugin_config::config_yaml_path(dir, plugin_id)))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn grants_mtime_for(manifest_path: &std::path::Path) -> SystemTime {
    manifest_path
        .parent()
        .map(|dir| mtime_or_epoch(&super::grants::grants_path(dir)))
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
mod agent_verb_tests;
#[cfg(test)]
mod converter_tests;
#[cfg(test)]
mod keep_on_failure_tests;
#[cfg(test)]
mod recording_tests;
#[cfg(test)]
mod surface_tests;
#[cfg(test)]
mod tests;
