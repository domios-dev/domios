//! Background poller for plugin root changes. It hot-reloads modified plugins
//! and registers or deregisters plugin folders without restarting the daemon.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;

use crate::agent::registry::AgentRegistry;

use super::{load_one, PluginManifest};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Per-plugin state the watcher tracks so it can tell `add` from
/// `modify` from `nothing-to-do` on each tick.
#[derive(Clone)]
struct Tracked {
    manifest_mtime: SystemTime,
    entry_mtime: SystemTime,
    /// mtime of the per-plugin `config.yaml`; an edit re-stamps `host.settingsJson` into the live VM.
    config_mtime: SystemTime,
}

#[derive(Clone)]
pub struct CapabilityRegistries {
    pub secret: Arc<crate::secrets::SecretRegistry>,
    pub transcribe: Arc<crate::transcribe::TranscribeRegistry>,
    pub glance: crate::glance::GlanceRegistryHandle,
    pub bubble: crate::bubble::BubbleRegistryHandle,
    pub view: crate::view_host::ViewRegistryHandle,
    pub chatbackend: crate::chatbackend::ChatBackendRegistryHandle,
    pub chat_preprocessor: crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
    pub agent_command: crate::agent_commands::AgentCommandRegistryHandle,
}

impl CapabilityRegistries {
    pub fn register_plugin(&self, manifest: &PluginManifest, vm: &Arc<super::PluginVm>) {
        let id = &manifest.id;
        let caps = &manifest.capabilities;
        if caps.transcriber {
            self.transcribe.register(
                id.clone(),
                Arc::new(super::JsTranscriber::new(vm.clone(), id.clone())),
            );
        }
        if caps.secret_backend {
            self.secret.register(
                id.clone(),
                Arc::new(super::JsSecretBackend::new(vm.clone(), id)),
            );
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
            crate::chatpipeline::warn_stage_conflicts(&self.transcribe, &self.chat_preprocessor);
        }
    }

    pub fn deregister_plugin(&self, id: &str) {
        self.transcribe.unregister(id);
        self.secret.unregister(id);
        self.glance.unregister(id);
        self.bubble.unregister(id);
        self.view.unregister(id);
        self.chatbackend.unregister(id);
        self.chat_preprocessor.unregister(id);
        self.agent_command.unregister(id);
    }
}

/// Live registry of discovered JS plugins. Spawn `start` to begin
/// watching the configured roots.
pub struct HotReloadRegistry {
    roots: Vec<PathBuf>,
    agent_registry: Arc<AgentRegistry>,
    caps: CapabilityRegistries,
    state: Mutex<HashMap<String, Tracked>>,
    vms: Mutex<HashMap<String, std::sync::Weak<super::PluginVm>>>,
    activated_startup: Mutex<HashSet<String>>,
    pending_reload: Mutex<HashSet<String>>,
    service: Mutex<Option<std::sync::Weak<crate::service::ServiceContext>>>,
    #[cfg(test)]
    load_count: std::sync::atomic::AtomicUsize,
}

/// Process-wide handle to the live watcher, so stateless surfaces can query `pending_reload` without threading the Arc.
static WATCHER: OnceLock<Arc<HotReloadRegistry>> = OnceLock::new();

/// Plugin ids installed after daemon start that need a reload to activate.
pub fn pending_reload_ids() -> Vec<String> {
    WATCHER
        .get()
        .map(|w| w.pending_reload_ids())
        .unwrap_or_default()
}

/// Late-bind the `ServiceContext` into the running watcher (it starts before the service exists). No-op before `start()`.
pub fn set_watcher_service(svc: std::sync::Weak<crate::service::ServiceContext>) {
    if let Some(w) = WATCHER.get() {
        w.set_service_ref(svc);
    }
}

impl HotReloadRegistry {
    pub fn new(
        roots: Vec<PathBuf>,
        agent_registry: Arc<AgentRegistry>,
        caps: CapabilityRegistries,
    ) -> Arc<Self> {
        Arc::new(Self {
            roots,
            agent_registry,
            caps,
            state: Mutex::new(HashMap::new()),
            vms: Mutex::new(HashMap::new()),
            activated_startup: Mutex::new(HashSet::new()),
            pending_reload: Mutex::new(HashSet::new()),
            service: Mutex::new(None),
            #[cfg(test)]
            load_count: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn set_service_ref(&self, svc: std::sync::Weak<crate::service::ServiceContext>) {
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

    /// The live VM for `id`, or `None` once the plugin is gone (the caller then rebuilds).
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

    /// Plugin ids installed after start that need a reload to activate (§1.8).
    pub fn pending_reload_ids(&self) -> Vec<String> {
        self.pending_reload.lock().iter().cloned().collect()
    }

    /// The capability registry handles threaded into the watcher (§1.8).
    pub fn capability_registries(&self) -> &CapabilityRegistries {
        &self.caps
    }

    fn emit_agent_provider_changed(&self) {
        let svc = self.service.lock().clone().and_then(|w| w.upgrade());
        if let Some(svc) = svc {
            svc.events
                .publish(crate::api::events::SystemEvent::ContributionsChanged {
                    kind: "agent_provider".to_string(),
                });
        }
    }

    /// Spawn the background poller. The handle owns itself — no need
    /// for callers to keep it.
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
        // Collect removed ids under a short-held `state` lock, then drop it before
        // touching registries below it in the load order — holding `state` across them risks a deadlock.
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
            provider_changed |= self.agent_registry.unregister_provider(&id);
            provider_changed |= self.caps.chatbackend.contains(&id);
            self.caps.deregister_plugin(&id);
            self.pending_reload.lock().remove(&id);
            self.activated_startup.lock().remove(&id);
            log::info!("[plugin-watcher] unregistered {id} (folder removed)");
        }
        if provider_changed {
            self.emit_agent_provider_changed();
        }
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
                        log::warn!("[plugin-watcher] {} parse: {e}", manifest_path.display());
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

    /// Returns the plugin id once it's been seen / registered / reloaded.
    fn reconcile_one(&self, manifest_path: &std::path::Path) -> Option<String> {
        let manifest_mtime = mtime_or_epoch(manifest_path);
        // Parse manifest to learn id + entry path. Cheap enough on a 2s poll; the alternative is caching mtime → id which adds a second source of truth.
        let manifest = match PluginManifest::load(manifest_path) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("[plugin-watcher] {} parse: {e}", manifest_path.display());
                return None;
            }
        };
        let entry_path = manifest.entry_path()?;
        if !entry_path.exists() {
            return None;
        }
        let entry_mtime = mtime_or_epoch(&entry_path);
        let config_mtime = config_mtime_for(manifest_path);

        // Snapshot prior state without holding the lock across I/O or QuickJS work — parking_lot is non-reentrant, and the VM mutex sits below this one in the lock order.
        let prior = self.state.lock().get(&manifest.id).cloned();
        if let Some(prev) = prior {
            let manifest_changed = prev.manifest_mtime != manifest_mtime;
            let entry_changed = prev.entry_mtime != entry_mtime;
            let config_changed = prev.config_mtime != config_mtime;
            if !manifest_changed && !entry_changed && !config_changed {
                return Some(manifest.id);
            }
            // A config-only change must NOT rebuild the VM (that wipes in-memory sessions);
            // re-stamp settings into the existing VM instead. Only a manifest/entry change rebuilds.
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
                // No live VM to re-stamp — fall through to a full load.
            }
            // Full rebuild on a manifest/entry change: in-place reload would race a provider call holding the VM mutex.
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
                // Track the VM so a later config-only change can re-stamp settings into it.
                self.register_vm(&id, provider.vm());
                // A `requiresReload` plugin installed after start can't hot-activate: mark pending, skip live registration.
                let activated_at_startup = self.activated_startup.lock().contains(&id);
                if provider.manifest().requires_reload && !activated_at_startup {
                    self.pending_reload.lock().insert(id.clone());
                    self.state.lock().insert(
                        id.clone(),
                        Tracked {
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
                // Bind the ServiceContext before registering capabilities — host.mem/worker/agent must be live before an adapter can call them.
                self.bind_service(provider.vm());
                // Re-register capability adapters live so an installed capability plugin works without a restart.
                self.caps
                    .register_plugin(provider.manifest(), provider.vm());
                // Only agent plugins register as providers; a capability-only plugin must not pollute the agent registry.
                if provider.manifest().capabilities.agent_provider {
                    // Registry handle so the JS provider can delegate unimplemented capabilities to its native fallback.
                    provider.set_registry(self.agent_registry.clone());
                    self.agent_registry.register_provider(Box::new(provider));
                    log::info!("[plugin-watcher] registered {id}");
                    self.emit_agent_provider_changed();
                } else {
                    if provider.manifest().capabilities.chat_backend_spawnable() {
                        self.emit_agent_provider_changed();
                    }
                    log::info!(
                        "[plugin-watcher] {id} capability registered live; not an agent provider — skipped agent registry"
                    );
                }
                self.state.lock().insert(
                    id.clone(),
                    Tracked {
                        manifest_mtime,
                        entry_mtime,
                        config_mtime,
                    },
                );
                crate::plugin::audit::append_daemon_reloaded(&id, &version);
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
                None
            }
        }
    }

    /// Snapshot of currently tracked plugin ids. Test/diagnostic.
    pub fn tracked_ids(&self) -> Vec<String> {
        self.state.lock().keys().cloned().collect()
    }

    #[cfg(test)]
    pub fn load_count(&self) -> usize {
        self.load_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn mtime_or_epoch(p: &std::path::Path) -> SystemTime {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// mtime of the plugin's `config.yaml` sibling, or the epoch when absent — so a later create/edit registers as a change.
fn config_mtime_for(manifest_path: &std::path::Path) -> SystemTime {
    manifest_path
        .parent()
        .map(|dir| mtime_or_epoch(&super::plugin_config::config_yaml_path(dir)))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests;
