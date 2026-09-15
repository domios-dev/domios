//! Host trait abstractions — the only bridge between codeterm-plugin and the
//! app crate. All operations are JSON-in / JSON-out to keep the FFI boundary
//! clean and avoid leaking app types into this crate.

use crate::activity::PluginActivity;
use crate::provider::AgentProvider;

/// Combined host context: everything a plugin VM needs from the app side.
/// Implemented by `ServiceContext` (and test stubs). The VM holds a
/// `Weak<dyn PluginHostCtx>` so teardown doesn't pin the context alive.
pub trait PluginHostCtx: Send + Sync {
    // ── Memory ──────────────────────────────────────────────────────────
    fn mem_search_json(&self, opts_json: &str) -> String;

    // ── Analytics ───────────────────────────────────────────────────────
    /// Machine-wide KPI aggregates for one query; never a ledger handle or path.
    fn analytics_query_json(&self, query_json: &str) -> String;

    // ── Workspace / tab ────────────────────────────────────────────────
    fn workspace_ensure_json(&self, opts_json: &str) -> String;
    fn workspace_tabs_json(&self, workspace_id: &str) -> String;
    fn close_tab(&self, tab_id: &str);
    fn spawn_tab_json(&self, spec_json: &str) -> String;

    // ── Worker ──────────────────────────────────────────────────────────
    fn worker_get_json(&self, session_id: &str) -> Option<String>;
    fn worker_kill(&self, session_id: &str);
    fn worker_pick_color(&self) -> String;
    fn worker_register_json(&self, info_json: &str);
    fn worker_start_json(&self, plugin_id: &str, opts_json: &str) -> String;
    fn worker_poll_json(&self, session_id: &str) -> String;

    // ── Agent ───────────────────────────────────────────────────────────
    fn agent_spawn_json(&self, plugin_id: &str, workspace_id: &str, opts_json: &str) -> String;
    fn agent_send_json(&self, plugin_id: &str, session_id: &str, text: &str) -> String;
    fn agent_reap_json(&self, session_id: &str) -> String;
    fn agent_poll_json(&self, session_id: &str) -> String;

    // ── Plugin activity (progress) ───────────────────────────────────────
    fn progress_raw_json(&self, plugin_id: &str, opts_json: &str) -> String;

    // ── Events ──────────────────────────────────────────────────────────
    fn contributions_changed(&self, kind: &str);
    fn emit_graph_worker_spawned_json(&self, info_json: &str);

    // ── Socket / API ────────────────────────────────────────────────────
    fn socket_display_path(&self) -> String;
    fn api_base_url(&self) -> String;
    fn api_token(&self) -> Option<String>;

    // ── Secrets ─────────────────────────────────────────────────────────
    fn secret_get(&self, plugin_id: &str, name: &str) -> Option<String>;
    fn secret_set(&self, plugin_id: &str, name: &str, value: &str) -> bool;
    fn secret_delete(&self, plugin_id: &str, name: &str) -> bool;
}

/// Trait for the app's agent registry, used by `PluginManager` to register
/// providers contributed by native plugins on activation.
pub trait PluginProviderRegistrar: Send + Sync {
    fn register_provider(&self, provider: Box<dyn AgentProvider>);
    fn unregister_provider(&self, id: &str) -> bool;
    /// Return the native provider displaced by a JS drop-in with this id, if any.
    fn native_fallback(&self, id: &str) -> Option<std::sync::Arc<dyn AgentProvider>>;
    /// A provider plugin finished installing or upgrading and is registered.
    /// The host decides what that warrants; this crate owns no consent record.
    fn provider_reloaded(&self, _id: &str) {}
}

/// Event sink for `PluginManager` — fires `ContributionsChanged` when the
/// plugin surface changes (providers, skills, commands, …).
pub trait PluginEventSink: Send + Sync {
    fn contributions_changed(&self, kind: &str);
}

/// Notification that a plugin's activity changed. The app implements this to
/// emit to the Tauri frontend via `app_handle.emit`.
pub trait PluginActivitySink: Send + Sync {
    fn emit_plugin_activity(&self, activity: &PluginActivity);
}

/// Registers a JS plugin's secret backend with the app's `SecretRegistry`.
/// The app implements this; codeterm-plugin can't hold `SecretRegistry` types.
pub trait SecretBackendRegistrar: Send + Sync {
    /// `declares_auto_unlock` carries the manifest's `secretBackend.autoUnlock`
    /// declaration so the host branches on a capability instead of a plugin id.
    fn register_secret_backend(
        &self,
        plugin_id: String,
        vm: std::sync::Arc<crate::js_runtime::PluginVm>,
        declares_auto_unlock: bool,
    );
    fn unregister_secret_backend(&self, plugin_id: &str);
}
