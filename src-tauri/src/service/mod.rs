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

mod workspace;
mod pane;
pub mod pty_io;
mod hierarchy;
mod orchestrator;
mod notification;
mod output;
pub mod cron;
pub mod pty_lifecycle;
pub mod general_agent;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::agent::registry::AgentRegistry;
use crate::api::events::EventBroadcaster;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::AgentRole;
use crate::{AgentTrackerState, NotifState, OutputBufferState, PromptDetectorState, PtyState, ReportStoreState, ScreenStateHandle};

// Re-export public types used by callers.
pub use orchestrator::{BatchOp, SpawnAgentParams, SpawnResult};
pub use hierarchy::PaneStatusInfo;

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
    pub prompt_detector: PromptDetectorState,
    /// Channel for outbound Telegram messages (CLI bridge.send command).
    pub bridge_outbound: crate::bridge::manager::OutboundSender,
    /// Receiver half — taken when bridge starts. Replaced on restart with a new channel.
    pub bridge_outbound_rx: Arc<parking_lot::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<crate::bridge::manager::OutboundMessage>>>>,
    /// Handle to the running bridge task — aborted on restart.
    pub bridge_task: Arc<parking_lot::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

pub type ServiceState = Arc<ServiceContext>;

impl ServiceContext {
    // ── Event helpers (used by all sub-modules) ─────────────────────

    pub(crate) fn emit_state_changed(&self) {
        self.events.publish(crate::api::events::SystemEvent::MuxStateChanged);
        let handle = self.app_handle.clone();
        std::thread::spawn(move || {
            let _ = handle.emit("mux-state-changed", ());
        });
    }

    pub(crate) fn emit_pane_title_pinned(&self, pane_id: &str, title: &str) {
        self.events.publish(crate::api::events::SystemEvent::PaneTitlePinned {
            pane_id: pane_id.to_string(),
            title: title.to_string(),
        });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"pane_id": pane_id, "title": title});
        std::thread::spawn(move || {
            let _ = handle.emit("pane-title-pinned", payload);
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
            "Lead" => Ok(AgentRole::Lead),
            "Worker" | "SubWorker" => Ok(AgentRole::Worker),
            _ => Err(format!("Invalid role: {}. Use: Orchestrator, Lead, Worker", role_str)),
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
