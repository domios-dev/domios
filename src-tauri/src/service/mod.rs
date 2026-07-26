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
pub mod boot_stage;
mod boot_wait;
pub(crate) mod chat_send_transaction;
mod command_port_impl;
pub mod cron;
pub mod delivery_error;
pub mod general_agent;
pub mod health;
mod hierarchy;
mod inject_safety;
pub mod inject_verify;
pub mod input_parser;
mod kpi_hooks;
pub(crate) mod launch_delivery;
pub(crate) mod launch_diagnostics;
pub mod message_gateway;
pub mod messenger;
pub mod notification;
mod orchestration_pair;
mod orchestrator;
pub mod orchestrator_digests;
mod orchestrator_onboard;
mod output;
mod output_feed;
pub(crate) mod pane;
pub mod pane_budget;
pub mod pane_chat;
pub mod pane_transport;
pub mod pane_transport_chat;
pub(crate) mod pending_delivery_payloads;
pub mod plan;
mod plan_reconcile;
pub mod plugin_host;
pub(crate) mod pty_activation;
pub mod pty_io;
pub mod pty_lifecycle;
pub(crate) mod pty_ownership;
pub mod push;
pub mod remote_workers;
mod role_identity;
mod role_profile;
pub mod session_persist;
pub mod smart_add;
pub mod spawn;
mod spawn_chatbackend;
mod spawn_cli;
mod spawn_factory;
mod spawn_params;
mod spawn_plan_task;
pub(crate) mod spawn_title;
mod status_reconciliation;
pub mod store;
pub mod structured_prompt;
pub mod sync_watcher;
mod team;
#[cfg(test)]
pub(crate) use team::close_team_with_state_change;
#[cfg(test)]
pub(crate) use team::rename_team_core;
#[cfg(test)]
pub(crate) use team::rename_team_with_state_change;
#[cfg(test)]
mod tests;
pub mod tokens;
pub mod transport;
pub mod watcher;
mod worker_identity;
mod worker_lane;
#[cfg(test)]
pub(crate) use worker_lane::provision_lane_for_test;
pub(crate) use worker_lane::{verify_worker_lane, ProvisionedWorkerLane, WorkerLaneProvenance};
mod worker_persona;
mod workspace;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
pub(crate) use hierarchy::FirstPlanTeamError;
pub use hierarchy::PaneStatusInfo;
pub use inject_verify::{
    screen_contains_echo, snapshot_prompt_line, wait_for_submit_shell, InjectionError,
    InjectionResult, VerifyOpts,
};
pub use orchestration_pair::{OrchestrationPairParams, OrchestrationPairResult};
#[cfg(test)]
pub(crate) use orchestrator::{arm_provider_session_for_test, LaunchGeneration};
pub(crate) use orchestrator::{record_provider_session, ProviderSessionBinding};
pub use orchestrator::{
    BatchOp, RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning, SpawnAgentParams,
    SpawnIdentityProvenance, SpawnResult,
};
pub use pane::{normalize_pane_description, MAX_PANE_DESCRIPTION_CHARS};
pub use pane_budget::{evaluate_pane_budget, PaneBudgetLevel, PaneBudgetNotice};
pub(crate) use team::TeamMutationCaller;
pub(crate) use team::TeamPlanError;
pub(crate) use team::{AddTeamMemberParams, AddTeamMemberResult};
pub(crate) use worker_identity::{
    persist_attempt_executor_identity, persist_attempt_executor_identity_for_mux,
    respawn_title_provenance,
};
pub(crate) use worker_persona::WorkerPersonaReservations;

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
    /// Per-pty shell-integration capability accumulators (OSC 133/633/7 ->
    /// command list + cwd). Fed from `process_pty_output`; consumed by chat.
    pub shell_caps: crate::ShellCapabilityState,
    pub report_store: ReportStoreState,
    /// Orchestration-economics event writer. Disabled (and file-less) unless `kpi.enabled`.
    pub kpi: crate::agent::kpi::KpiRecorder,
    pub agent_registry: Arc<AgentRegistry>,
    pub mem: Arc<crate::mem::MemManager>,
    pub mem_supervisor: Arc<crate::mem::supervisor::Supervisor>,
    pub activity: Arc<crate::mem::activity::ActivityRing>,
    pub workers: Arc<crate::mem::workers::GraphWorkerRegistry>,
    pub plugin_activity: Arc<crate::plugin::activity::PluginActivityRegistry>,
    pub worker_pool: Arc<crate::mem::worker_pool::WorkerPool>,
    pub prompt_detector: PromptDetectorState,
    pub(crate) pty_ownership: pty_ownership::PtyOwnership,
    // Stable pane→pty map for hidden agent runs; survives close_pane so the unified AgentRun log endpoints can read closed-worker output.
    pub agent_run_pty_map: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
    // codeterm cron complete / mem worker report itself.
    pub run_watcher: Arc<crate::agent::run_watcher::RunWatcherRegistry>,
    /// Orchestrator-side view of remote (cross-mesh) workers, populated from
    /// inbound mesh reports. Drives the "remote pane" ghost nodes in the UI.
    pub remote_workers: crate::service::remote_workers::RemoteWorkerRegistryHandle,
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
    /// Per-pane locks serializing the chatBackend restore→reopen transition so a
    /// concurrent send/deliver can't double-open one pane's plugin session.
    pub chatbackend_reopen_guards: Arc<pane_transport_chat::PaneReopenGuards>,
    /// Unified facade over the agent + chat-backend stores. Listing and
    /// spawn-classify go through this so call-sites stop touching two registries.
    pub provider_registry: Arc<crate::provider::ProviderRegistry>,
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
    /// Evidence counters for the daemon→desktop event path (event-freeze probe).
    pub mux_emit_stats: Arc<MuxEmitStats>,
    /// Coalesces debounced session snapshots — see `schedule_session_persist_debounced`.
    pub session_persist_pending: Arc<AtomicBool>,
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
    pub user_last_activity: Arc<crate::user_last_activity::UserLastActivity>,
    pub usage_poller: Arc<parking_lot::Mutex<Option<crate::usage::UsagePoller>>>,
    /// Global plan store with project-scoped queries. Sole writer.
    pub plan_store: Arc<crate::service::plan::PlanStore>,
    pub plan_watchers: Arc<
        parking_lot::Mutex<
            std::collections::HashMap<
                std::path::PathBuf,
                crate::service::plan::watcher::PlanWatcher,
            >,
        >,
    >,
    pub loaded_plan_projects:
        Arc<parking_lot::Mutex<std::collections::HashSet<std::path::PathBuf>>>,
    pub plan_startup_loaded: Arc<AtomicBool>,
    pub plan_turn_counter: Arc<crate::service::plan::reminders::TurnCounter>,
    pub(crate) pending_delivery_payloads:
        Arc<pending_delivery_payloads::PendingDeliveryPayloadStore>,
    pub(crate) chat_send_ledger: Arc<chat_send_transaction::ChatSendLedger>,
    pub(crate) worker_identity_allocator:
        Arc<parking_lot::Mutex<worker_persona::WorkerPersonaReservations>>,
    /// The single optimistic outbound-delivery seam with idempotency.
    /// Callers migrate onto `gateway.submit(...)` incrementally (task_1iiwnc).
    pub(crate) gateway: Arc<crate::service::message_gateway::MessageGateway>,
}

pub type ServiceState = Arc<ServiceContext>;

/// Instrumentation for the desktop `mux-state-changed` emit path. `requests` =
/// `emit_state_changed` calls; `fires` = actual `app.emit` calls; `suppressed`
/// = coalesced early-returns; `pending_since_ms` = epoch ms of the last
/// false→true `mux_emit_pending` transition (0 once reset). A latched emit
/// shows requests climbing, fires flat, pending stuck true with a growing age.
#[derive(Default)]
pub struct MuxEmitStats {
    pub requests: AtomicU64,
    pub fires: AtomicU64,
    pub suppressed: AtomicU64,
    pub pending_since_ms: AtomicU64,
    /// Debug-only deterministic freeze (fault injection): when set, every
    /// `emit_state_changed` early-returns, reproducing the latched-emit symptom
    /// without a race against draining coalesce threads.
    pub frozen: AtomicBool,
}

fn wall_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    hierarchy::start_plan_reminder_scheduler(svc);
}

impl ServiceContext {
    pub(crate) fn create_pending_delivery(
        &self,
        pane_id: &str,
        channel: crate::multiplexer::types::PendingDeliveryChannel,
        payload: &str,
    ) -> Result<String, pending_delivery_payloads::PayloadStoreError> {
        use sha2::Digest;
        let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));
        let previous_digest = self
            .mux
            .read()
            .get_pane(pane_id)
            .and_then(|pane| pane.pending_delivery.as_ref())
            .map(|record| record.content_digest.clone());
        self.pending_delivery_payloads
            .replace(previous_digest.as_deref(), &digest, payload)?;
        let mut mux = self.mux.write();
        let pane = mux.get_pane_mut(pane_id).ok_or_else(|| {
            pending_delivery_payloads::PayloadStoreError::Io {
                digest: digest.clone(),
                message: format!("pane {pane_id} vanished while creating pending delivery"),
            }
        })?;
        pane.pending_delivery = Some(crate::multiplexer::types::PendingDeliveryRecord::new(
            digest.clone(),
            channel,
        ));
        drop(mux);
        crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
        .map_err(|message| pending_delivery_payloads::PayloadStoreError::Io {
            digest: digest.clone(),
            message,
        })?;
        Ok(digest)
    }

    pub(crate) fn resolve_pending_delivery(
        &self,
        pane_id: &str,
    ) -> Result<Option<String>, pending_delivery_payloads::PayloadStoreError> {
        use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryState};
        let pending = self
            .mux
            .read()
            .get_pane(pane_id)
            .and_then(|pane| pane.pending_delivery.clone());
        match pending {
            Some(record)
                if record.state() == PendingDeliveryState::Pending
                    && record.channel == PendingDeliveryChannel::InteractiveInput =>
            {
                self.pending_delivery_payloads
                    .resolve(&record.content_digest)
                    .map(Some)
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn confirm_pending_delivery(
        &self,
        pane_id: &str,
    ) -> Result<(), pending_delivery_payloads::PayloadStoreError> {
        use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryState};
        let digest = self
            .mux
            .read()
            .get_pane(pane_id)
            .and_then(|pane| pane.pending_delivery.as_ref())
            .filter(|record| record.channel == PendingDeliveryChannel::InteractiveInput)
            .map(|record| record.content_digest.clone());
        let Some(digest) = digest else { return Ok(()) };
        let mut mux = self.mux.write();
        let pane = mux.get_pane_mut(pane_id).ok_or_else(|| {
            pending_delivery_payloads::PayloadStoreError::Io {
                digest: digest.clone(),
                message: "pane vanished while confirming pending delivery".into(),
            }
        })?;
        if let Some(record) = pane.pending_delivery.as_mut() {
            if record.state() == PendingDeliveryState::Pending {
                record.confirm().map_err(|error| {
                    pending_delivery_payloads::PayloadStoreError::Io {
                        digest: record.content_digest.clone(),
                        message: format!("{error:?}"),
                    }
                })?;
            }
        }
        drop(mux);
        crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
        .map_err(|message| pending_delivery_payloads::PayloadStoreError::Io {
            digest: digest.clone(),
            message,
        })?;
        self.pending_delivery_payloads.delete(&digest)?;
        Ok(())
    }
    // ── Event helpers (used by all sub-modules) ─────────────────────

    /// Notify the UI that an orchestrator's remote (cross-mesh) workers changed.
    /// One payload shape for every mutation site (spawn registration, UI relay
    /// hook, inbound report) so the sidebar refreshes consistently.
    pub(crate) fn emit_remote_workers_changed(&self, orchestrator_pane: &str) {
        // SSE bus, so the web mirror updates live too — not only on next remount.
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "remote-workers-changed",
            serde_json::json!({
                "orchestrator_pane_id": orchestrator_pane,
                "workers": self.remote_workers.snapshot(),
            }),
        );
    }

    pub(crate) fn emit_state_changed(&self) {
        publish_mux_state_changed(&self.events);
        self.emit_state_changed_after_bus_publish();
    }

    pub(crate) fn emit_state_changed_after_bus_publish(&self) {
        self.mux_emit_stats.requests.fetch_add(1, Ordering::Relaxed);
        #[cfg(debug_assertions)]
        if self.mux_emit_stats.frozen.load(Ordering::Acquire) {
            self.mux_emit_stats
                .suppressed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        // Coalesce frontend emits to ≤1/50ms — output.rs fires this 4× per PTY chunk.
        if self.mux_emit_pending.swap(true, Ordering::AcqRel) {
            self.mux_emit_stats
                .suppressed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.mux_emit_stats
            .pending_since_ms
            .store(wall_now_ms(), Ordering::Relaxed);
        let handle = self.app_handle.clone();
        let pending = self.mux_emit_pending.clone();
        let mux = self.mux.clone();
        let agent_tracker = self.agent_tracker.clone();
        let agent_registry = self.agent_registry.clone();
        let stats = self.mux_emit_stats.clone();
        let spawned = std::thread::Builder::new()
            .name("mux-emit".into())
            .spawn(move || {
                let snapshot = Self::coalesce_snapshot(
                    &mux,
                    &agent_tracker,
                    &agent_registry,
                    &pending,
                    std::time::Duration::from_millis(50),
                );
                stats.pending_since_ms.store(0, Ordering::Relaxed);
                if !stats.frozen.load(Ordering::Acquire) {
                    stats.fires.fetch_add(1, Ordering::Relaxed);
                    let _ = handle.emit("mux-state-changed", &snapshot);
                }
            });
        // Evidence build: surface a spawn failure instead of silently latching.
        // The latch (pending stuck true) is intentionally left in place to observe.
        if let Err(e) = spawned {
            log::error!("[events] mux coalesce thread spawn FAILED: {e} — emit latch STUCK");
        }
    }

    /// Emit an empty-payload `mux-state-changed` (frontend does a full refetch).
    /// The socket handlers use this so fault injection (`frozen`) covers EVERY
    /// desktop mux-state-changed path, not just the coalesced `emit_state_changed`.
    pub(crate) fn emit_mux_state_changed_empty(&self) {
        self.mux_emit_stats.requests.fetch_add(1, Ordering::Relaxed);
        #[cfg(debug_assertions)]
        if self.mux_emit_stats.frozen.load(Ordering::Acquire) {
            self.mux_emit_stats
                .suppressed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        let handle = self.app_handle.clone();
        let stats = self.mux_emit_stats.clone();
        std::thread::spawn(move || {
            stats.fires.fetch_add(1, Ordering::Relaxed);
            let _ = handle.emit("mux-state-changed", ());
        });
    }

    /// Sleep out the coalesce window, then read a fresh snapshot. The pending
    /// flag is cleared BEFORE the read so any mutation landing after this point
    /// opens a new window (its own emit) rather than being swallowed; mutations
    /// that landed during the window are captured by reading last.
    ///
    /// Uses `snapshot_with_status` (not the plain `snapshot`) — this is the
    /// ONLY path that keeps a browser tab's `paneStatuses` fresh between page
    /// loads. Without it, `pane_statuses` here is always empty and the
    /// reducer's merge (`incoming ?? keep-stale`) leaves a synthetic `Waiting`
    /// (set by `/prompt/clear`'s pending_prompt-derived override) stuck
    /// forever once the underlying PTY stops transitioning Idle/Working.
    fn coalesce_snapshot(
        mux: &MuxState,
        agent_tracker: &AgentTrackerState,
        agent_registry: &AgentRegistry,
        pending: &AtomicBool,
        delay: std::time::Duration,
    ) -> crate::multiplexer::state::MultiplexerSnapshot {
        std::thread::sleep(delay);
        pending.store(false, Ordering::Release);
        status_reconciliation::snapshot_with_reconciled_status(mux, agent_tracker, agent_registry)
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
        self.pty_ownership.release_ptys(pty_ids);
        let mut manager = self.pty.lock();
        for id in pty_ids {
            let _ = manager.kill(id);
        }
    }

    pub fn ensure_plan_project_loaded(
        &self,
        project_path: &std::path::Path,
    ) -> Result<std::path::PathBuf, crate::service::plan::PlanError> {
        let project_root = crate::service::plan::paths::project_root(project_path);
        let should_scan = {
            let loaded = self.loaded_plan_projects.lock();
            !loaded.contains(&project_root)
        };
        if should_scan {
            self.plan_store.scan_project(&project_root)?;
            self.loaded_plan_projects
                .lock()
                .insert(project_root.clone());
            self.reconcile_dangling_team_plan_bindings(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )?;
        }
        let legacy_plans_dir = project_root.join(".codeterm").join("plans");
        let mut watchers = self.plan_watchers.lock();
        if legacy_plans_dir.is_dir() && !watchers.contains_key(&project_root) {
            match crate::service::plan::watcher::PlanWatcher::watch_project(
                &project_root,
                self.plan_store.clone(),
                std::sync::Arc::new(crate::service::plan::sink_impl::TauriPlanEventSink::new(
                    self.app_handle.clone(),
                )),
            ) {
                Ok(watcher) => {
                    watchers.insert(project_root.clone(), watcher);
                }
                Err(e) => log::warn!("[plan] failed to watch {}: {}", project_root.display(), e),
            }
        }
        Ok(project_root)
    }

    pub fn plans_startup_loaded(&self) -> bool {
        self.plan_startup_loaded.load(Ordering::Acquire)
    }

    pub fn mark_plans_startup_loaded(&self) {
        self.plan_startup_loaded.store(true, Ordering::Release);
        self.notify_event("plans-loaded", serde_json::json!({}));
    }

    /// Unified event dispatch — sends to Tauri frontend and external webhooks.
    /// OS notifications are NOT sent here — use in-app toasts only.
    pub fn notify_event(&self, event_type: &str, payload: serde_json::Value) {
        // 1. Native webview + SSE bus (web mirror) through the single seam.
        crate::api::events::emit_everywhere(&self.app_handle, event_type, &payload);
        // 2. External webhooks + websockets (Telegram, Discord, etc.)
        self.events.broadcast(event_type, payload);
    }

    pub(crate) fn parse_role(role_str: &str) -> Result<AgentRole, String> {
        match role_str {
            "Orchestrator" | "Conductor" => Ok(AgentRole::Orchestrator),
            "Manager" => Ok(AgentRole::Manager),
            "Worker" => Ok(AgentRole::Worker),
            _ => Err(format!(
                "Invalid role: {}. Use: Orchestrator, Manager, Worker",
                role_str
            )),
        }
    }
}

pub(crate) fn publish_mux_state_changed(events: &crate::api::events::EventBroadcaster) {
    events.publish(crate::api::events::SystemEvent::MuxStateChanged);
}

// ── Shared data types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReportResult {
    pub report_id: String,
    pub orchestrator_pane_id: String,
    /// Non-fatal notices from plan/task resolution (stale env ids, auto-resolution).
    pub notices: Vec<String>,
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
