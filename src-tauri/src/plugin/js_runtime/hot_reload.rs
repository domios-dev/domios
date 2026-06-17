//! Background poller that watches the plugin roots for `plugin.js` /
//! `plugin.json` changes and either hot-reloads the existing VM or
//! discovers + registers a brand-new plugin folder — all without
//! restarting the daemon.
//!
//! Polling (every 2s) instead of inotify/ReadDirectoryChangesW because
//! the cross-platform `notify` backend config in this crate currently
//! only enables macos_fsevent. Polling N files at 2s intervals is
//! trivially cheap for the volumes here (1-10 plugins).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
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
}

/// The non-agent capability registries the watcher needs so it can re-register
/// a (re)loaded plugin's capabilities live — mirroring the startup arms in
/// `lib.rs` (plan §1.8). C0 only threads these handles in; the actual
/// re-registration on reload (and `chatBackend`) lands in a later track.
#[derive(Clone)]
pub struct CapabilityRegistries {
    pub secret: Arc<crate::secrets::SecretRegistry>,
    pub transcribe: Arc<crate::transcribe::TranscribeRegistry>,
    pub glance: crate::glance::GlanceRegistryHandle,
    pub bubble: crate::bubble::BubbleRegistryHandle,
    pub view: crate::view_host::ViewRegistryHandle,
    pub chatbackend: crate::chatbackend::ChatBackendRegistryHandle,
    pub chat_preprocessor: crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
}

/// Live registry of discovered JS plugins. Spawn `start` to begin
/// watching the configured roots.
pub struct HotReloadRegistry {
    roots: Vec<PathBuf>,
    agent_registry: Arc<AgentRegistry>,
    /// Capability registry handles, carried so a later track can re-register
    /// reloaded plugins' capabilities without a daemon restart (§1.8).
    caps: CapabilityRegistries,
    /// Plugin id → tracked state. Mutex because the watcher mutates
    /// from a background thread.
    state: Mutex<HashMap<String, Tracked>>,
    #[cfg(test)]
    load_count: std::sync::atomic::AtomicUsize,
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
            #[cfg(test)]
            load_count: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// The capability registry handles threaded into the watcher (§1.8).
    pub fn capability_registries(&self) -> &CapabilityRegistries {
        &self.caps
    }

    /// Spawn the background poller. The handle owns itself — no need
    /// for callers to keep it.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("codeterm-plugin-watcher".into())
            .spawn(move || loop {
                me.tick();
                std::thread::sleep(POLL_INTERVAL);
            })
            .ok();
    }

    /// Run one scan-and-reconcile pass. Public so callers can force a
    /// rescan (tests; manual reload command).
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
        let mut state = self.state.lock();
        let removed: Vec<String> = state
            .keys()
            .filter(|id| !seen.contains(id))
            .cloned()
            .collect();
        for id in removed {
            state.remove(&id);
            self.agent_registry.unregister_provider(&id);
            log::info!("[plugin-watcher] unregistered {id} (folder removed)");
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

        // Snapshot prior state without holding the lock across I/O or QuickJS work — parking_lot is non-reentrant, and the VM mutex sits below this one in the lock order.
        let prior = self.state.lock().get(&manifest.id).cloned();
        if let Some(prev) = prior {
            let manifest_changed = prev.manifest_mtime != manifest_mtime;
            let entry_changed = prev.entry_mtime != entry_mtime;
            if !manifest_changed && !entry_changed {
                return Some(manifest.id);
            }
            // Always full-rebuild on any change. In-place `vm.reload` would be faster but adds a second code path that races against any provider call
            // holding the VM mutex (an idle pane's detect_from_output still runs at PTY-event rate). Throwing the old VM away and constructing a new one is 10-50ms — cheap enough for plugin edits.
            log::info!(
                "[plugin-watcher] {} changed (manifest={manifest_changed}, entry={entry_changed}) — reloading",
                manifest.id
            );
        }
        self.full_load(manifest_path, manifest_mtime, entry_mtime)
    }

    fn full_load(
        &self,
        manifest_path: &std::path::Path,
        manifest_mtime: SystemTime,
        entry_mtime: SystemTime,
    ) -> Option<String> {
        match load_one(manifest_path) {
            Ok(mut provider) => {
                let id = provider.manifest().id.clone();
                let version = provider.manifest().version.clone();
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
                } else {
                    log::info!(
                        "[plugin-watcher] {id} is a capability plugin (not an agent) — skipped agent registration"
                    );
                }
                self.state.lock().insert(
                    id.clone(),
                    Tracked {
                        manifest_mtime,
                        entry_mtime,
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
