//! Plugin system skeleton.
//!
//! This module defines the contract and lifecycle for CodeTerm plugins.
//! It is intentionally minimal right now: the manifest parser, the
//! `Plugin` trait, and the `PluginManager` shell exist so that future
//! work (extracting agent-claude as the first native plugin, process
//! plugins over JSON-RPC, WASM sandboxing) can plug in without touching
//! core modules.
//!
//! See `docs/plans/plugin-architecture.md` for the full design.

pub mod agent_claude;
pub mod agent_codex;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::api::events::SystemEvent;

/// Runtime tier a plugin ships as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginTier {
    /// Compiled into the binary, full ServiceContext access, zero IPC.
    Native,
    /// Long-running external process spoken to over JSON-RPC stdio.
    Process,
    /// Sandboxed WebAssembly module (wasm32-wasip2).
    Wasm,
}

/// What a plugin contributes to the host.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    /// CLI subcommands registered under `codeterm <name>`.
    #[serde(default)]
    pub commands: Vec<String>,
    /// MCP-style tool identifiers exposed to agents.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Custom pane renderers this plugin provides.
    #[serde(default)]
    pub pane_types: Vec<String>,
    /// Agent provider ids this plugin registers.
    #[serde(default)]
    pub agent_providers: Vec<String>,
    /// Event hook names this plugin subscribes to.
    #[serde(default)]
    pub hooks: Vec<String>,
    /// Plugin has a settings UI panel.
    #[serde(default)]
    pub settings: bool,
}

/// Permission declarations the plugin needs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub mesh_sync: bool,
}

/// When the plugin should be activated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginActivation {
    #[serde(default)]
    pub triggers: Vec<String>,
}

/// Prompt-section contributions for the General Agent prompt and skills.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPrompt {
    #[serde(default)]
    pub ga_section: Option<String>,
    #[serde(default)]
    pub skill_section: Option<String>,
}

/// Parsed `plugin.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub activation: PluginActivation,
    #[serde(default)]
    pub prompt: PluginPrompt,
    /// Absolute path the manifest was loaded from, filled in by the loader.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub tier: PluginTier,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub wasm_module: Option<String>,
    /// If omitted, plugin is considered universal.
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub min_codeterm: Option<String>,
}

impl PluginManifest {
    /// Parse a manifest from a `plugin.toml` file on disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let mut manifest: PluginManifest = toml::from_str(&text)
            .map_err(|e| format!("failed to parse {}: {}", path.display(), e))?;
        manifest.source_path = Some(path.to_path_buf());
        Ok(manifest)
    }

    /// Whether this plugin should run on the current host platform.
    pub fn matches_platform(&self) -> bool {
        if self.plugin.platforms.is_empty() {
            return true;
        }
        let current = std::env::consts::OS;
        self.plugin.platforms.iter().any(|p| p == current)
    }
}

/// A universal skill file contributed by a plugin. Each provider knows
/// how to deliver it (Claude writes to `~/.claude/skills/`, Codex merges
/// into the system prompt, etc.).
#[derive(Debug, Clone)]
pub struct SkillFile {
    pub plugin_id: String,
    pub name: String,
    pub markdown: String,
    pub triggers: Vec<String>,
}

/// Plugin lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    Discovered,
    Active,
    Failed,
}

/// Runtime state for a single registered plugin.
pub struct PluginEntry {
    pub manifest: PluginManifest,
    pub state: PluginState,
    pub error: Option<String>,
    pub plugin: Option<Box<dyn Plugin>>,
}

/// Core plugin trait implemented by native plugins (process/WASM plugins
/// use adapter wrappers that implement this trait on their behalf).
pub trait Plugin: Send + Sync {
    /// Stable plugin identifier, must match manifest.plugin.name.
    fn id(&self) -> &str;

    /// Called once after discovery when the plugin is activated.
    fn activate(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Called once when the plugin is being shut down.
    fn deactivate(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Called for every SystemEvent the plugin is subscribed to.
    /// Default implementation does nothing.
    fn on_event(&mut self, _event: &SystemEvent) {}

    /// Return agent providers contributed by this plugin.
    /// Called once during activation — providers are registered into AgentRegistry.
    /// Default: no providers.
    fn agent_providers(&self) -> Vec<Box<dyn crate::agent::provider::AgentProvider>> {
        Vec::new()
    }
}

/// Plugin registry and lifecycle manager. Holds the set of known
/// plugins, their runtime state, and a subscription to the EventBus
/// for dispatching events to active plugins.
pub struct PluginManager {
    entries: RwLock<HashMap<String, PluginEntry>>,
    agent_registry: Option<Arc<crate::agent::registry::AgentRegistry>>,
    /// Event broadcaster for emitting `ContributionsChanged` when a plugin's
    /// contribution surface (agent providers, skills, commands, ...) changes.
    /// Optional so tests can construct a manager without a live event bus.
    /// The Tauri-side bridge subscribes to the bus and forwards
    /// `ContributionsChanged` to the frontend — the manager itself never
    /// touches `tauri::AppHandle` (keeps test linkage clean of webview deps).
    events: Option<Arc<crate::api::events::EventBroadcaster>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            agent_registry: None,
            events: None,
        }
    }

    /// Set the agent registry for provider registration during plugin activation.
    pub fn set_agent_registry(&mut self, registry: Arc<crate::agent::registry::AgentRegistry>) {
        self.agent_registry = Some(registry);
    }

    /// Wire the event broadcaster so contribution changes notify in-process
    /// subscribers (and through them, the Tauri frontend via the bus bridge
    /// installed in `lib.rs`).
    pub fn set_event_broadcaster(
        &mut self,
        events: Arc<crate::api::events::EventBroadcaster>,
    ) {
        self.events = Some(events);
    }

    /// Emit `ContributionsChanged { kind }` to in-process subscribers.
    /// `kind` identifies which contribution category changed: "agent_provider",
    /// "skill", "command", etc. Frontend hooks filter by kind and refetch
    /// the matching list. The Tauri bridge forwards this event to JS-land.
    fn emit_contributions_changed(&self, kind: &str) {
        if let Some(ref events) = self.events {
            events.publish(crate::api::events::SystemEvent::ContributionsChanged {
                kind: kind.to_string(),
            });
        }
    }

    /// Scan a directory for `<dir>/*/plugin.toml` manifests and register
    /// each one in the `Discovered` state. Returns the number of plugins
    /// discovered. Errors from individual manifests are logged and skipped.
    pub fn discover<P: AsRef<Path>>(&self, root: P) -> Result<usize, String> {
        let root = root.as_ref();
        if !root.exists() {
            return Ok(0);
        }
        let mut count = 0;
        let entries = std::fs::read_dir(root)
            .map_err(|e| format!("failed to read {}: {}", root.display(), e))?;
        for entry in entries.flatten() {
            let manifest_path = entry.path().join("plugin.toml");
            if !manifest_path.exists() {
                continue;
            }
            match PluginManifest::load(&manifest_path) {
                Ok(manifest) => {
                    if !manifest.matches_platform() {
                        log::info!(
                            "[plugin] skipping {} — platform mismatch",
                            manifest.plugin.name
                        );
                        continue;
                    }
                    self.register(manifest);
                    count += 1;
                }
                Err(e) => log::warn!("[plugin] skipping {}: {}", manifest_path.display(), e),
            }
        }
        Ok(count)
    }

    /// Register a manifest without activating it. Overwrites any prior
    /// entry with the same name.
    pub fn register(&self, manifest: PluginManifest) {
        let name = manifest.plugin.name.clone();
        let entry = PluginEntry {
            manifest,
            state: PluginState::Discovered,
            error: None,
            plugin: None,
        };
        self.entries.write().insert(name, entry);
    }

    /// Attach a native plugin implementation to a previously registered
    /// manifest. Fails if the manifest is not registered or has a
    /// non-native tier.
    pub fn attach_native(&self, plugin: Box<dyn Plugin>) -> Result<(), String> {
        let id = plugin.id().to_string();
        let mut guard = self.entries.write();
        let entry = guard
            .get_mut(&id)
            .ok_or_else(|| format!("plugin {} is not registered", id))?;
        if entry.manifest.plugin.tier != PluginTier::Native {
            return Err(format!(
                "plugin {} is not declared as native (tier={:?})",
                id, entry.manifest.plugin.tier
            ));
        }
        entry.plugin = Some(plugin);
        Ok(())
    }

    /// Activate a plugin by id. Calls `Plugin::activate`, registers any
    /// contributed agent providers, and transitions to `Active` on success
    /// or `Failed` on error.
    pub fn activate(&self, id: &str) -> Result<(), String> {
        let mut contributed_providers = false;
        let result = {
            let mut guard = self.entries.write();
            let entry = guard
                .get_mut(id)
                .ok_or_else(|| format!("plugin {} is not registered", id))?;
            if let Some(plugin) = entry.plugin.as_mut() {
                match plugin.activate() {
                    Ok(()) => {
                        // Register agent providers contributed by this plugin
                        if let Some(ref registry) = self.agent_registry {
                            for provider in plugin.agent_providers() {
                                registry.register_provider(provider);
                                contributed_providers = true;
                            }
                        }
                        entry.state = PluginState::Active;
                        entry.error = None;
                        Ok(())
                    }
                    Err(e) => {
                        entry.state = PluginState::Failed;
                        entry.error = Some(e.clone());
                        Err(e)
                    }
                }
            } else {
                // Process / WASM plugins: activation is a no-op at this level
                // until the respective hosts are implemented.
                entry.state = PluginState::Active;
                Ok(())
            }
        };
        if result.is_ok() && contributed_providers {
            self.emit_contributions_changed("agent_provider");
        }
        result
    }

    /// Deactivate a plugin. Idempotent: returns Ok on unknown ids.
    pub fn deactivate(&self, id: &str) -> Result<(), String> {
        let mut removed_providers = false;
        {
            let mut guard = self.entries.write();
            let Some(entry) = guard.get_mut(id) else {
                return Ok(());
            };
            // Unregister agent providers contributed by this plugin
            if let Some(ref registry) = self.agent_registry {
                for provider_id in &entry.manifest.capabilities.agent_providers {
                    if registry.unregister_provider(provider_id) {
                        removed_providers = true;
                    }
                }
            }
            if let Some(plugin) = entry.plugin.as_mut() {
                plugin.deactivate()?;
            }
            entry.state = PluginState::Discovered;
            entry.error = None;
        }
        if removed_providers {
            self.emit_contributions_changed("agent_provider");
        }
        Ok(())
    }

    /// Names of all registered plugins (any state).
    pub fn list(&self) -> Vec<String> {
        self.entries.read().keys().cloned().collect()
    }

    /// Names of currently-active plugins.
    pub fn active(&self) -> Vec<String> {
        self.entries
            .read()
            .iter()
            .filter(|(_, e)| e.state == PluginState::Active)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Snapshot the state of one plugin.
    pub fn state_of(&self, id: &str) -> Option<PluginState> {
        self.entries.read().get(id).map(|e| e.state)
    }

    /// Dispatch a SystemEvent to every active native plugin whose manifest
    /// declares the event in its `hooks` list (or whose hooks list is empty,
    /// meaning subscribe-to-all).
    pub fn dispatch_event(&self, event: &SystemEvent) {
        let event_type = event.type_name();
        let mut guard = self.entries.write();
        for entry in guard.values_mut() {
            if entry.state != PluginState::Active {
                continue;
            }
            let hooks = &entry.manifest.capabilities.hooks;
            if !hooks.is_empty() && !hooks.iter().any(|h| h == event_type) {
                continue;
            }
            if let Some(plugin) = entry.plugin.as_mut() {
                plugin.on_event(event);
            }
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}


pub type PluginManagerState = Arc<PluginManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn manifest(name: &str, tier: PluginTier) -> PluginManifest {
        PluginManifest {
            plugin: PluginMeta {
                name: name.into(),
                version: "0.1.0".into(),
                description: String::new(),
                tier,
                entrypoint: None,
                wasm_module: None,
                platforms: vec![],
                min_codeterm: None,
            },
            capabilities: PluginCapabilities::default(),
            permissions: PluginPermissions::default(),
            activation: PluginActivation::default(),
            prompt: PluginPrompt::default(),
            source_path: None,
        }
    }

    struct DummyPlugin {
        id: String,
        activations: Arc<AtomicUsize>,
        events: Arc<AtomicUsize>,
    }

    impl Plugin for DummyPlugin {
        fn id(&self) -> &str {
            &self.id
        }
        fn activate(&mut self) -> Result<(), String> {
            self.activations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn on_event(&mut self, _event: &SystemEvent) {
            self.events.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn register_and_list() {
        let mgr = PluginManager::new();
        mgr.register(manifest("a", PluginTier::Native));
        mgr.register(manifest("b", PluginTier::Process));
        let mut names = mgr.list();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(mgr.state_of("a"), Some(PluginState::Discovered));
    }

    #[test]
    fn attach_native_then_activate() {
        let mgr = PluginManager::new();
        mgr.register(manifest("counter", PluginTier::Native));
        let activations = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(AtomicUsize::new(0));
        mgr.attach_native(Box::new(DummyPlugin {
            id: "counter".into(),
            activations: activations.clone(),
            events: events.clone(),
        }))
        .unwrap();
        mgr.activate("counter").unwrap();
        assert_eq!(activations.load(Ordering::SeqCst), 1);
        assert_eq!(mgr.state_of("counter"), Some(PluginState::Active));
        assert_eq!(mgr.active(), vec!["counter".to_string()]);
    }

    #[test]
    fn attach_native_rejects_non_native_tier() {
        let mgr = PluginManager::new();
        mgr.register(manifest("proc", PluginTier::Process));
        let err = mgr
            .attach_native(Box::new(DummyPlugin {
                id: "proc".into(),
                activations: Arc::new(AtomicUsize::new(0)),
                events: Arc::new(AtomicUsize::new(0)),
            }))
            .unwrap_err();
        assert!(err.contains("not declared as native"));
    }

    #[test]
    fn dispatch_event_filters_by_hooks() {
        let mgr = PluginManager::new();
        let mut m_all = manifest("all", PluginTier::Native);
        let mut m_filtered = manifest("filtered", PluginTier::Native);
        m_filtered.capabilities.hooks = vec!["pane-title-pinned".into()];
        mgr.register(m_all);
        mgr.register(m_filtered);

        let all_events = Arc::new(AtomicUsize::new(0));
        let filtered_events = Arc::new(AtomicUsize::new(0));
        mgr.attach_native(Box::new(DummyPlugin {
            id: "all".into(),
            activations: Arc::new(AtomicUsize::new(0)),
            events: all_events.clone(),
        }))
        .unwrap();
        mgr.attach_native(Box::new(DummyPlugin {
            id: "filtered".into(),
            activations: Arc::new(AtomicUsize::new(0)),
            events: filtered_events.clone(),
        }))
        .unwrap();
        mgr.activate("all").unwrap();
        mgr.activate("filtered").unwrap();

        mgr.dispatch_event(&SystemEvent::MuxStateChanged);
        mgr.dispatch_event(&SystemEvent::PaneTitlePinned {
            pane_id: "p".into(),
            title: "t".into(),
        });

        assert_eq!(all_events.load(Ordering::SeqCst), 2);
        assert_eq!(filtered_events.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn deactivate_idempotent() {
        let mgr = PluginManager::new();
        assert!(mgr.deactivate("nope").is_ok());
        mgr.register(manifest("p", PluginTier::Process));
        mgr.activate("p").unwrap();
        mgr.deactivate("p").unwrap();
        assert_eq!(mgr.state_of("p"), Some(PluginState::Discovered));
    }

    #[test]
    fn manifest_load_from_toml_string() {
        let toml = r#"
            [plugin]
            name = "memory"
            version = "0.1.0"
            description = "Semantic memory"
            tier = "process"
            entrypoint = "python3 main.py"
            platforms = ["windows", "macos", "linux"]

            [capabilities]
            tools = ["memory.save", "memory.search"]
            commands = ["memory"]
            hooks = ["mux-state-changed"]

            [permissions]
            filesystem = ["~/.codeterm/plugins/memory/"]
            network = false

            [activation]
            triggers = ["on_startup"]

            [prompt]
            skill_section = "prompts/memory.md"
        "#;
        let parsed: PluginManifest = toml::from_str(toml).unwrap();
        assert_eq!(parsed.plugin.name, "memory");
        assert_eq!(parsed.plugin.tier, PluginTier::Process);
        assert_eq!(parsed.capabilities.tools.len(), 2);
        assert_eq!(parsed.capabilities.hooks, vec!["mux-state-changed"]);
        assert_eq!(parsed.activation.triggers, vec!["on_startup"]);
        assert_eq!(parsed.prompt.skill_section.as_deref(), Some("prompts/memory.md"));
    }

    #[test]
    fn matches_platform_empty_means_universal() {
        let m = manifest("u", PluginTier::Native);
        assert!(m.matches_platform());
    }

    #[test]
    fn discover_scans_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
                [plugin]
                name = "memory"
                version = "0.1.0"
                tier = "process"
            "#,
        )
        .unwrap();

        let mgr = PluginManager::new();
        let count = mgr.discover(tmp.path()).unwrap();
        assert_eq!(count, 1);
        assert_eq!(mgr.list(), vec!["memory".to_string()]);
    }

    #[test]
    fn discover_on_missing_path_returns_zero() {
        let mgr = PluginManager::new();
        let count = mgr.discover("/definitely/does/not/exist/codeterm-plugins").unwrap();
        assert_eq!(count, 0);
    }
}
