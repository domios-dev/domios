//! Unified service layer for CodeTerm multiplexer operations.
//!
//! Both REST API routes, Tauri IPC commands, and the Socket server
//! delegate to this module. This ensures consistent behavior (PTY cleanup,
//! event emission, hierarchy validation) regardless of the entry point.
//!
//! Sub-modules group operations by domain:
//! - `workspace` — workspace & tab CRUD
//! - `pane` — pane lifecycle, split, rename, focus, browser
//! - `pty_io` — send text, batch send, PTY helpers
//! - `hierarchy` — agent roles, reports, status queries
//! - `orchestrator` — agent spawn, batch commands
//! - `notification` — create notification with OS toast

pub mod activity;
pub mod agent_restart;
pub mod cron;
pub mod general_agent;
pub mod health;
mod hierarchy;
mod inject_safety;
pub mod inject_verify;
pub mod input_parser;
pub mod msg_prefix;
pub mod notification;
mod orchestrator;
mod output;
mod pane;
pub mod pane_chat;
pub mod pane_transport;
mod pane_transport_chat;
pub mod pty_io;
pub mod pty_lifecycle;
pub mod push;
pub mod smart_add;
pub mod spawn;
mod spawn_chatbackend;
mod spawn_cli;
mod spawn_title;
pub mod store;
pub mod sync_watcher;
#[cfg(test)]
mod tests;
pub mod tokens;
mod workspace;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::agent::registry::AgentRegistry;
use crate::api::events::EventBroadcaster;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::AgentRole;
use crate::{
    AgentTrackerState, NotifState, OutputBufferState, PromptDetectorState, PtyState,
    ReportStoreState, ScreenStateHandle,
};

// Re-export public types used by callers.
pub use hierarchy::PaneStatusInfo;
pub use inject_verify::{
    screen_contains_echo, snapshot_prompt_line, wait_for_echo, wait_for_submit_shell,
    wait_for_submit_tui_drained, InjectionError, InjectionResult, VerifyOpts,
};
pub use orchestrator::{BatchOp, SpawnAgentParams, SpawnResult};

/// Shared service context — all state needed for multiplexer operations.
/// Both API routes, Tauri commands, and Socket server hold a clone of this.
#[derive(Clone)]
pub struct ServiceContext {
    pub mux: MuxState,
    pub pty: PtyState,
    pub output_buf: OutputBufferState,
    pub screen_state: ScreenStateHandle,
    pub notif: NotifState,
    pub events: Arc<EventBroadcaster>,
    pub app_handle: tauri::AppHandle,
    pub agent_tracker: AgentTrackerState,
    pub report_store: ReportStoreState,
    pub agent_registry: Arc<AgentRegistry>,
    pub mem: Arc<crate::mem::MemManager>,
    pub mem_supervisor: Arc<crate::mem::supervisor::Supervisor>,
    pub activity: Arc<crate::mem::activity::ActivityRing>,
    pub workers: Arc<crate::mem::workers::GraphWorkerRegistry>,
    pub plugin_activity: Arc<crate::plugin::activity::PluginActivityRegistry>,
    pub worker_pool: Arc<crate::mem::worker_pool::WorkerPool>,
    pub prompt_detector: PromptDetectorState,
    // Stable pane→pty map for hidden agent runs; survives close_pane so the unified AgentRun log endpoints can read closed-worker output.
    pub agent_run_pty_map: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
    // Auto-completes hidden agent runs from claude stream-json result events, idle silence, or hard timeout — replaces relying on claude calling
    // codeterm cron complete / mem worker report itself.
    pub run_watcher: Arc<crate::agent::run_watcher::RunWatcherRegistry>,
    /// Channel for outbound Telegram messages (CLI bridge.send command).
    pub bridge_outbound: crate::bridge::manager::OutboundSender,
    /// Receiver half — taken when bridge starts. Replaced on restart with a new channel.
    pub bridge_outbound_rx: Arc<
        parking_lot::Mutex<
            Option<tokio::sync::mpsc::UnboundedReceiver<crate::bridge::manager::OutboundMessage>>,
        >,
    >,
    /// Supervisor for the messaging bridge — owns task handle, cancel signal, runtime state.
    pub bridge_supervisor: Arc<crate::bridge::supervisor::BridgeSupervisor>,
    /// Per-pane last-restart timestamp for `agent_restart` rate limiting.
    pub restart_tracker: agent_restart::RestartTracker,
    /// Per-pane JSONL chat tailers — populated when a pane enters UI mode.
    pub chat_registry: Arc<crate::service::pane_chat::ChatRegistry>,
    pub delayed_send: crate::delayed_send::DelayedSendQueue,
    /// Active secret-store backend. Built-ins via factory; plugin backends
    /// (e.g. Bitwarden) come from `secret_registry`.
    pub secrets: crate::secrets::SecretStoreState,
    /// Registry of plugin-contributed secret backends, keyed by id.
    pub secret_registry: Arc<crate::secrets::SecretRegistry>,
    /// Registry of transcribers (built-in HTTP engine + any Transcriber plugin).
    /// Voice notes resolve the active transcriber here by id.
    pub transcribe_registry: Arc<crate::transcribe::TranscribeRegistry>,
    /// Registry of glance-view providers (plugins with `glanceView`). Pinned
    /// plugins render their popover from here.
    pub glance_registry: crate::glance::GlanceRegistryHandle,
    /// Registry of status-bubble providers (plugins with `statusBubble`). The
    /// pane-footer bubble row polls these per pane context.
    pub bubble_registry: crate::bubble::BubbleRegistryHandle,
    /// Registry of view providers (plugins with `view`). The iframe bridge
    /// (`ct.invoke`) routes here, gated by the plugin's permissions.
    pub view_registry: crate::view_host::ViewRegistryHandle,
    /// Registry of chatBackend providers (plugins with `chatBackend`). Non-PTY
    /// panes route their I/O here via the `PaneTransport` seam.
    pub chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle,
    /// Registry of chatPreprocessor providers (plugins with `chatPreprocessor`).
    /// `run_outbound_stages` runs these send-time text stages over every user
    /// turn before dispatching to transport.
    pub chat_preprocessor_registry: crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
    /// Registry of agent-command handlers (plugins with the `agent_commands`
    /// grant). The socket dispatch routes `plugin:<id>.<verb>` here — the inverse
    /// seam letting a spawned agent drive a plugin's `onAgentCommand`.
    pub agent_command_registry: crate::agent_commands::AgentCommandRegistryHandle,
    /// Coalesces `mux-state-changed` emits — see `emit_state_changed`.
    pub mux_emit_pending: Arc<AtomicBool>,
    pub services_store: Arc<crate::service::store::ServicesStore>,
    pub tokens: Arc<crate::service::tokens::TokenStore>,
    /// Ephemeral cross-mesh pastebin. In-memory only; daemon restart wipes
    /// all pastes. 20-entry cap, oldest-first eviction, TTL≤24h. (#98)
    pub pasteboard: crate::pasteboard::PasteboardState,
    /// Watches bytes about to be written to PTYs; promotes typed `claude/codex/
    /// opencode ...<Enter>` lines to a locked `agent_type` on the pane.
    pub input_parser: Arc<input_parser::InputParser>,
    pub usage_store: crate::usage::UsageStoreHandle,
    pub usage_activity: crate::usage::PaneActivity,
    pub usage_poller: Arc<parking_lot::Mutex<Option<crate::usage::UsagePoller>>>,
}

pub type ServiceState = Arc<ServiceContext>;

impl ServiceContext {
    // ── Event helpers (used by all sub-modules) ─────────────────────

    pub(crate) fn emit_state_changed(&self) {
        self.events
            .publish(crate::api::events::SystemEvent::MuxStateChanged);
        // Coalesce frontend emits to ≤1/50ms — output.rs fires this 4× per PTY chunk.
        if self.mux_emit_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let handle = self.app_handle.clone();
        let pending = self.mux_emit_pending.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            pending.store(false, Ordering::Release);
            let _ = handle.emit("mux-state-changed", ());
        });
    }

    pub(crate) fn emit_pane_title_pinned(&self, pane_id: &str, title: &str) {
        self.events
            .publish(crate::api::events::SystemEvent::PaneTitlePinned {
                pane_id: pane_id.to_string(),
                title: title.to_string(),
            });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"pane_id": pane_id, "title": title});
        std::thread::spawn(move || {
            let _ = handle.emit("pane-title-pinned", payload);
        });
    }

    /// Emit a focus-change event to the frontend. Delayed 75 ms so it always
    /// arrives AFTER the coalesced 50 ms `mux-state-changed` window — this way
    /// the frontend's structural state is up to date before it tries to apply
    /// focus to (potentially newly created) panes. Frontend listener also has a
    /// short retry as defense-in-depth.
    pub(crate) fn emit_pane_focus_changed(&self, pane_id: &str) {
        self.events
            .publish(crate::api::events::SystemEvent::PaneFocused {
                pane_id: pane_id.to_string(),
            });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"pane_id": pane_id});
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            let _ = handle.emit("pane-focused", payload);
        });
    }

    pub(crate) fn kill_ptys(&self, pty_ids: &[String]) {
        let mut manager = self.pty.lock();
        for id in pty_ids {
            let _ = manager.kill(id);
        }
    }

    /// Unified event dispatch — sends to Tauri frontend and external webhooks.
    /// OS notifications are NOT sent here — use in-app toasts only.
    pub fn notify_event(&self, event_type: &str, payload: serde_json::Value) {
        // 1. Tauri event to frontend (in-app toasts)
        let _ = self.app_handle.emit(event_type, &payload);

        // 2. External webhooks + websockets (Telegram, Discord, etc.)
        self.events.broadcast(event_type, payload);
    }

    pub(crate) fn parse_role(role_str: &str) -> Result<AgentRole, String> {
        match role_str {
            "Orchestrator" | "Conductor" => Ok(AgentRole::Orchestrator),
            "Worker" => Ok(AgentRole::Worker),
            _ => Err(format!(
                "Invalid role: {}. Use: Orchestrator, Worker",
                role_str
            )),
        }
    }
}

// ── Shared data types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReportResult {
    pub report_id: String,
    pub orchestrator_pane_id: String,
}

/// Helper: extract optional string from JSON value.
pub(crate) fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Replace "$ref" strings in params with actual pane_ids from previous results.
pub(crate) fn resolve_refs(
    params: &serde_json::Value,
    refs: &std::collections::HashMap<String, String>,
) -> serde_json::Value {
    match params {
        serde_json::Value::String(s) if s.starts_with('$') => {
            let key = &s[1..];
            refs.get(key)
                .map(|v| serde_json::Value::String(v.clone()))
                .unwrap_or_else(|| params.clone())
        }
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), resolve_refs(v, refs));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| resolve_refs(v, refs)).collect())
        }
        other => other.clone(),
    }
}
