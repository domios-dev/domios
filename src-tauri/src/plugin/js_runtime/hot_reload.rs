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
    /// mtime of the per-plugin `config.yaml` (§1.7). An edit triggers a reload
    /// that re-stamps `host.settingsJson` into the VM — live settings reload.
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

/// Process-wide handle to the live watcher, set when `start()` runs. Lets
/// stateless surfaces (the plugin snapshot command, CLI-routed `get plugin`)
/// query `pending_reload` without threading the Arc through Tauri state.
static WATCHER: OnceLock<Arc<HotReloadRegistry>> = OnceLock::new();

/// Plugin ids installed after daemon start that need a reload to activate
/// (§1.8). Empty before the watcher starts or on a fresh process.
pub fn pending_reload_ids() -> Vec<String> {
    WATCHER
        .get()
        .map(|w| w.pending_reload_ids())
        .unwrap_or_default()
}

/// Late-bind the `ServiceContext` into the running watcher (P4). Called once
/// from `lib.rs` after service init: the watcher starts before the service
/// exists, so it can't receive the ref at construction. No-op before `start()`.
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

    /// The live VM for `id`, if still alive. `None` once the plugin is gone — the
    /// caller then rebuilds rather than re-stamping a dead VM.
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
            svc.events.publish(crate::api::events::SystemEvent::ContributionsChanged {
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
        // Anything we used to track that's no longer present → unregister.
        let mut provider_changed = false;
        let mut state = self.state.lock();
        let removed: Vec<String> = state
            .keys()
            .filter(|id| !seen.contains(id))
            .cloned()
            .collect();
        for id in removed {
            state.remove(&id);
            self.vms.lock().remove(&id);
            provider_changed |= self.agent_registry.unregister_provider(&id);
            provider_changed |= self.caps.chatbackend.contains(&id);
            self.caps.deregister_plugin(&id);
            self.pending_reload.lock().remove(&id);
            self.activated_startup.lock().remove(&id);
            log::info!("[plugin-watcher] unregistered {id} (folder removed)");
        }
        drop(state);
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
            // A CONFIG-ONLY change (e.g. the Settings UX writing config.yaml) must
            // NOT rebuild the VM: a rebuild wipes the plugin's module-scope
            // in-memory sessions, silently bricking every open chatBackend shell.
            // Re-stamp the resolved settings into the EXISTING VM instead, so
            // `host.settingsJson()` reflects the edit on its next call while live
            // sessions survive. Only a manifest/entry (contract/code) change
            // forces a full rebuild.
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
                // No live VM to re-stamp (never loaded / torn down) — fall through
                // to a full load so the new settings still take effect.
            }
            // Full-rebuild on a manifest/entry change. In-place `vm.reload` would
            // be faster but adds a second code path that races against any
            // provider call holding the VM mutex (an idle pane's
            // detect_from_output still runs at PTY-event rate). Throwing the old
            // VM away and constructing a new one is 10-50ms — cheap enough for
            // code edits.
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
                // Track the freshly built VM so a later config-only change can
                // re-stamp settings into it instead of rebuilding (§1.7 / HIGH-2).
                self.register_vm(&id, provider.vm());
                // A `requiresReload` plugin freshly installed after start cannot
                // hot-activate (§1.8): mark it pending and skip live capability
                // registration. Already-activated (startup) plugins reload normally.
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
                // Late-bind the ServiceContext into the new VM BEFORE registering
                // capabilities — host.mem/worker/agent must be live the moment a
                // capability adapter (e.g. chatPreprocessor's mem search) can be
                // called (P4). Startup-loaded plugins get this via lib.rs's
                // plugin_vms loop; hot-installed ones get it here.
                self.bind_service(provider.vm());
                // Re-register capability adapters live (§1.8 P1) — bubble/glance/
                // view/secret/transcribe/chatBackend/chatPreprocessor/agent_commands
                // — so an installed capability plugin works without a restart.
                self.caps
                    .register_plugin(provider.manifest(), provider.vm());
                // Only AGENT plugins register as providers. A capability-only
                // plugin (transcriber / secret backend) must NOT pollute the
                // agent registry — its adapter is registered at startup. Mirrors
                // the same capability gate in lib.rs (spec §2 invariant).
                if provider.manifest().capabilities.agent_provider {
                    // Registry handle so the (re)loaded JS provider can delegate
                    // unimplemented capabilities to its native fallback.
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

/// mtime of the plugin's `config.yaml` sibling, or the epoch when absent — so a
/// later `config.yaml` create/edit registers as a change and reloads the VM
/// (live settings reload, §1.7).
fn config_mtime_for(manifest_path: &std::path::Path) -> SystemTime {
    manifest_path
        .parent()
        .map(|dir| mtime_or_epoch(&super::plugin_config::config_yaml_path(dir)))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_caps() -> CapabilityRegistries {
        CapabilityRegistries {
            secret: Arc::new(crate::secrets::SecretRegistry::new()),
            transcribe: Arc::new(crate::transcribe::TranscribeRegistry::new()),
            glance: Arc::new(crate::glance::GlanceRegistry::new()),
            bubble: Arc::new(crate::bubble::BubbleRegistry::new()),
            view: Arc::new(crate::view_host::ViewRegistry::new()),
            chatbackend: Arc::new(crate::chatbackend::ChatBackendRegistry::new()),
            chat_preprocessor: Arc::new(crate::chatpreprocessor::ChatPreprocessorRegistry::new()),
            agent_command: Arc::new(crate::agent_commands::AgentCommandRegistry::new()),
        }
    }

    #[test]
    fn watcher_holds_threaded_capability_registries() {
        // C0: the watcher must carry the existing capability registry handles so
        // a later track can re-register a (re)loaded plugin's non-agent
        // capabilities live (plan §1.8). Prove the *same* handles are threaded in.
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let caps = test_caps();
        let watcher =
            HotReloadRegistry::new(vec![root.path().to_path_buf()], registry, caps.clone());
        let held = watcher.capability_registries();
        assert!(Arc::ptr_eq(&held.secret, &caps.secret));
        assert!(Arc::ptr_eq(&held.transcribe, &caps.transcribe));
        assert!(Arc::ptr_eq(&held.glance, &caps.glance));
        assert!(Arc::ptr_eq(&held.bubble, &caps.bubble));
        assert!(Arc::ptr_eq(&held.view, &caps.view));
        assert!(Arc::ptr_eq(&held.chatbackend, &caps.chatbackend));
        assert!(Arc::ptr_eq(
            &held.chat_preprocessor,
            &caps.chat_preprocessor
        ));
    }

    #[test]
    fn watcher_binds_service_into_freshly_loaded_vm() {
        // P4: a plugin installed/reloaded AFTER daemon start must get the
        // ServiceContext late-bound into its VM, or host.mem/worker/agent are
        // dead for it — only the startup loop bound the service before this fix.
        use std::sync::Weak;
        let watcher = HotReloadRegistry::new(vec![], Arc::new(AgentRegistry::new()), test_caps());
        watcher.set_service_ref(Weak::new());
        let vm = Arc::new(
            crate::plugin::js_runtime::PluginVm::from_source(
                "p",
                "module.exports.default = {};".into(),
            )
            .unwrap(),
        );
        assert!(
            watcher.bind_service(&vm),
            "watcher binds the service ref into the new VM"
        );
        assert!(
            !vm.set_service(Weak::new()),
            "already bound — the watcher already populated the service ref"
        );
    }

    #[test]
    fn watcher_bind_service_noop_without_ref() {
        let watcher = HotReloadRegistry::new(vec![], Arc::new(AgentRegistry::new()), test_caps());
        let vm = Arc::new(
            crate::plugin::js_runtime::PluginVm::from_source(
                "p",
                "module.exports.default = {};".into(),
            )
            .unwrap(),
        );
        assert!(
            !watcher.bind_service(&vm),
            "no service ref set → nothing to bind"
        );
    }

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn manifest_for(id: &str) -> String {
        use crate::plugin::js_runtime::host_api::HOST_API_VERSION;
        format!(
            r#"{{
                "id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
                "entry":"plugin.js"
            }}"#
        )
    }

    /// A capability-only plugin manifest (agent_provider defaults false because a
    /// `capabilities` block is present). `requires_reload` toggles the §1.8 flag.
    fn capability_manifest_for(id: &str, cap_field: &str) -> String {
        capability_manifest_full(id, cap_field, false)
    }

    fn capability_manifest_full(id: &str, cap_field: &str, requires_reload: bool) -> String {
        use crate::plugin::js_runtime::host_api::HOST_API_VERSION;
        format!(
            r#"{{
                "id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
                "capabilities":{{"{cap_field}":true}},
                "requiresReload":{requires_reload},
                "entry":"plugin.js"
            }}"#
        )
    }

    #[test]
    fn requires_reload_plugin_installed_live_is_pending_not_active() {
        // §1.8 P1: a plugin declaring requiresReload cannot hot-activate. A fresh
        // install after daemon start is marked pending_reload and its capability
        // is NOT registered live — the banner/CLI tell the user to reload.
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let caps = test_caps();
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            caps.clone(),
        );
        // No seed() — simulates a fresh install after startup (id not activated
        // at startup).
        let dir = root.path().join("needsreload");
        write(
            &dir.join("plugin.json"),
            &capability_manifest_full("needsreload", "statusBubble", true),
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");

        watcher.tick();

        assert!(
            !caps.bubble.contains("needsreload"),
            "requiresReload capability must NOT hot-activate"
        );
        assert!(
            watcher
                .pending_reload_ids()
                .contains(&"needsreload".to_string()),
            "fresh requiresReload install must be pending_reload"
        );

        // Uninstall clears the pending flag.
        std::fs::remove_dir_all(&dir).unwrap();
        watcher.tick();
        assert!(!watcher
            .pending_reload_ids()
            .contains(&"needsreload".to_string()));
    }

    #[test]
    fn seeded_requires_reload_plugin_is_not_pending() {
        // A plugin present at startup (seeded as activated) is already active from
        // the full process start — a later reload must NOT mark it pending.
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let caps = test_caps();
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            caps.clone(),
        );
        let dir = root.path().join("startupreload");
        write(
            &dir.join("plugin.json"),
            &capability_manifest_full("startupreload", "statusBubble", true),
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        // Seed: mark as activated-at-startup (lib.rs already registered it).
        watcher.seed();
        assert!(!watcher
            .pending_reload_ids()
            .contains(&"startupreload".to_string()));

        // Edit triggers a reload — still not pending (it was active at startup).
        std::thread::sleep(Duration::from_millis(1100));
        write(&dir.join("plugin.js"), "module.exports.default = { v: 2 };");
        watcher.tick();
        assert!(!watcher
            .pending_reload_ids()
            .contains(&"startupreload".to_string()));
    }

    fn plugin_for(id: &str) -> String {
        format!(
            r#"module.exports.default = {{
                detectFromTitle: function (t) {{ return t.indexOf("{id}") !== -1; }}
            }};"#
        )
    }

    #[test]
    fn tick_registers_new_plugins_added_after_start() {
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );
        // First tick: empty root, nothing to register.
        watcher.tick();
        assert!(watcher.tracked_ids().is_empty());
        assert_eq!(registry.provider_count(), 0);

        // Drop a plugin in.
        let dir = root.path().join("foo");
        write(&dir.join("plugin.json"), &manifest_for("foo"));
        write(&dir.join("plugin.js"), &plugin_for("foo"));

        watcher.tick();
        assert_eq!(watcher.tracked_ids(), vec!["foo".to_string()]);
        assert_eq!(registry.provider_count(), 1);
        assert!(registry
            .with_provider("foo", |p| p.detect_from_title("foo cli"))
            .unwrap_or(false));
    }

    #[test]
    fn tick_hot_reloads_plugin_js_changes() {
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );
        let dir = root.path().join("bar");
        write(&dir.join("plugin.json"), &manifest_for("bar"));
        write(&dir.join("plugin.js"), &plugin_for("v1"));
        watcher.tick();
        assert!(registry
            .with_provider("bar", |p| p.detect_from_title("v1"))
            .unwrap_or(false));
        assert!(!registry
            .with_provider("bar", |p| p.detect_from_title("v2"))
            .unwrap_or(true));

        // Sleep a tick so mtime resolution can register the change.
        std::thread::sleep(Duration::from_millis(1100));
        write(&dir.join("plugin.js"), &plugin_for("v2"));
        watcher.tick();

        // Same provider Arc<PluginVm> now serves new JS.
        assert!(registry
            .with_provider("bar", |p| p.detect_from_title("v2"))
            .unwrap_or(false));
        assert!(!registry
            .with_provider("bar", |p| p.detect_from_title("v1"))
            .unwrap_or(true));
    }

    #[test]
    fn duplicate_roots_only_load_the_winning_plugin() {
        let workspace = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );

        let workspace_dir = workspace.path().join("provider-dupe");
        write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
        write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
        let user_dir = user.path().join("dupe");
        write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
        write(&user_dir.join("plugin.js"), &plugin_for("user"));

        watcher.tick();
        assert_eq!(watcher.load_count(), 1);
        assert!(registry
            .with_provider("dupe", |p| p.detect_from_title("user cli"))
            .unwrap_or(false));
        assert!(!registry
            .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
            .unwrap_or(true));

        watcher.tick();
        assert_eq!(
            watcher.load_count(),
            1,
            "unchanged duplicate roots must not ping-pong reload"
        );
    }

    #[test]
    fn duplicate_root_removal_falls_back_to_lower_priority_plugin() {
        let workspace = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );

        let workspace_dir = workspace.path().join("provider-dupe");
        write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
        write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
        let user_dir = user.path().join("dupe");
        write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
        write(&user_dir.join("plugin.js"), &plugin_for("user"));

        watcher.tick();
        assert!(registry
            .with_provider("dupe", |p| p.detect_from_title("user cli"))
            .unwrap_or(false));

        std::fs::remove_dir_all(&user_dir).unwrap();
        watcher.tick();

        assert!(registry
            .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
            .unwrap_or(false));
        assert!(!registry
            .with_provider("dupe", |p| p.detect_from_title("user cli"))
            .unwrap_or(true));
    }

    #[test]
    fn tick_registers_capability_plugin_live_without_restart() {
        // §1.8 P1: installing a capability-only plugin must re-register its
        // capability registry live, so e.g. a status bubble appears without a
        // daemon restart (the git-bubble-dark-until-restart symptom).
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let caps = test_caps();
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            caps.clone(),
        );
        let dir = root.path().join("bubbler");
        write(
            &dir.join("plugin.json"),
            &capability_manifest_for("bubbler", "statusBubble"),
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");

        watcher.tick();

        // Capability-only plugin: NOT an agent provider, but its bubble
        // capability is live in the registry.
        assert_eq!(registry.provider_count(), 0);
        assert!(
            caps.bubble.contains("bubbler"),
            "status bubble must register live on install"
        );
    }

    #[test]
    fn tick_deregisters_capability_on_folder_removal() {
        // §1.8 P1: uninstall must deregister the capability — no ghost bubble.
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let caps = test_caps();
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            caps.clone(),
        );
        let dir = root.path().join("bubbler");
        write(
            &dir.join("plugin.json"),
            &capability_manifest_for("bubbler", "statusBubble"),
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        watcher.tick();
        assert!(caps.bubble.contains("bubbler"));

        std::fs::remove_dir_all(&dir).unwrap();
        watcher.tick();
        assert!(
            !caps.bubble.contains("bubbler"),
            "capability must be deregistered on uninstall"
        );
    }

    #[test]
    fn config_yaml_edit_restamps_settings_without_vm_rebuild() {
        // HIGH-2 (CRITICAL): editing the per-plugin config.yaml must NOT rebuild
        // the VM — a rebuild wipes the plugin's module-scope in-memory sessions,
        // silently bricking every open chatBackend shell. Instead the watcher
        // re-stamps host.settingsJson() into the EXISTING VM. Assert: a
        // config-only change does NOT increment load_count (no rebuild), AND the
        // live VM observes the new settings on its next read.
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );
        let dir = root.path().join("cfgplugin");
        write(&dir.join("plugin.json"), &manifest_for("cfgplugin"));
        write(
            &dir.join("plugin.js"),
            r#"module.exports.default = {
                detectFromTitle: function (t) { return false; },
                settings: function () { return JSON.parse(host.settingsJson()); }
            };"#,
        );
        write(&dir.join("config.yaml"), "engine: whisper\n");
        watcher.tick();
        assert_eq!(watcher.load_count(), 1);

        // The watcher tracks the live VM; it reads the initial settings.
        let vm = watcher
            .existing_vm("cfgplugin")
            .expect("VM tracked after load");
        let before: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
        assert_eq!(before["engine"], "whisper");

        // No change → no reload, no re-stamp.
        watcher.tick();
        assert_eq!(watcher.load_count(), 1);

        // Edit config.yaml only → re-stamp into the live VM, NOT a rebuild.
        std::thread::sleep(Duration::from_millis(1100));
        write(&dir.join("config.yaml"), "engine: deepgram\n");
        watcher.tick();
        assert_eq!(
            watcher.load_count(),
            1,
            "a config-only edit must NOT rebuild the VM (a rebuild bricks live sessions)"
        );

        // Same VM Arc, new settings visible — proves the re-stamp reached the
        // existing VM rather than a fresh one.
        let after: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
        assert_eq!(
            after["engine"], "deepgram",
            "re-stamped settings must be live on the existing VM"
        );

        // A second config-only edit still no-ops the rebuild path.
        std::thread::sleep(Duration::from_millis(1100));
        write(&dir.join("config.yaml"), "engine: vosk\n");
        watcher.tick();
        assert_eq!(watcher.load_count(), 1);
        let again: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
        assert_eq!(again["engine"], "vosk");
    }

    #[test]
    fn tick_unregisters_plugins_after_folder_removal() {
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(AgentRegistry::new());
        let watcher = HotReloadRegistry::new(
            vec![root.path().to_path_buf()],
            registry.clone(),
            test_caps(),
        );
        let dir = root.path().join("baz");
        write(&dir.join("plugin.json"), &manifest_for("baz"));
        write(&dir.join("plugin.js"), &plugin_for("baz"));
        watcher.tick();
        assert_eq!(registry.provider_count(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
        watcher.tick();
        assert!(watcher.tracked_ids().is_empty());
        assert_eq!(registry.provider_count(), 0);
    }
}
