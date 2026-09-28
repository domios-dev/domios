//! Unified service layer for CodeTerm multiplexer operations.
//!
//! Both REST API routes, Tauri IPC commands, and the Socket server
//! delegate to this module. This ensures consistent behavior (PTY cleanup,
//! event emission, hierarchy validation) regardless of the entry point.
//!
//! Sub-modules group operations by domain:
//! - `workspace` — workspace & tab CRUD
//! - `tab` — tab lifecycle, split, rename, focus, browser
//! - `pty_io` — send text, batch send, PTY helpers
//! - `hierarchy` — agent roles, reports, status queries
//! - `orchestrator` — agent spawn, batch commands
//! - `notification` — create notification with OS toast

pub mod activity;
pub mod agent_pad;
pub mod agent_restart;
mod agent_transition;
pub(crate) mod ai_helper_spawn;
mod analytics_host;
pub mod boot_stage;
pub(crate) mod boot_wait;
pub(crate) mod bridge_threads;
pub(crate) mod cartographer;
pub(crate) mod chat_send_evidence;
pub(crate) mod chat_send_transaction;
mod command_port_impl;
pub mod cron;
pub mod delivery_error;
pub mod general_agent;
pub mod health;
pub(crate) mod hierarchy;
pub mod inject_verify;
pub mod input_parser;
pub(crate) mod integration_silence;
mod kpi_hooks;
pub(crate) mod lane_removal;
pub(crate) mod launch_artifacts;
pub(crate) mod launch_delivery;
pub(crate) mod launch_diagnostics;
pub(crate) mod launch_input_gate;
pub(crate) mod launch_readiness;
pub(crate) mod launch_session_detection;
pub mod message_gateway;
pub mod messenger;
pub mod notification;
mod notification_desktop;
pub(crate) mod notification_push;
mod orchestration_pair;
mod orchestrator;
mod parked_user_delivery;
mod process_tree;
mod prompt_resolution;
pub(crate) mod public_send_readiness;
pub(crate) mod startup_turn;
#[cfg(test)]
pub(crate) use orchestrator::role_onboarding_task;
pub(crate) use orchestrator::{
    build_agent_command_for_service, launch_agent_in_background, mark_tab_agent_launch_started,
    provider_launch_task,
};
pub(crate) mod directory_trust;
pub mod orchestrator_digests;
mod orchestrator_onboard;
mod output;
mod output_feed;
pub(crate) mod output_stream_activity;
pub(crate) mod pending_delivery_payloads;
pub(crate) mod permission_auto_confirm;
pub(crate) mod permission_policy;
pub(crate) mod persona;
pub mod plan;
mod plan_authority;
pub(crate) mod plan_batch_authority;
mod plan_pause;
mod plan_reconcile;
pub mod plugin_host;
pub(crate) mod provider_identity;
pub(crate) mod pty_activation;
pub mod pty_io;
pub mod pty_lifecycle;
pub(crate) mod pty_ownership;
pub mod push;
mod reasoning_effort;
pub(crate) mod receiver_target;
pub mod remote_tabs;
pub mod remote_workers;
pub(crate) mod restore_exited_member;
mod restored_binding;
mod role_identity;
pub(crate) mod role_preset;
pub(crate) mod role_profile;
mod session_backup;
pub(crate) mod session_binding_deferral;
mod session_binding_state;
pub(crate) mod session_model;
pub mod session_persist;
mod shell_restart;
pub mod smart_add;
pub mod spawn;
pub(crate) mod spawn_capability;
mod spawn_chatbackend;
mod spawn_cli;
mod spawn_factory;
mod spawn_params;
mod spawn_plan_task;
pub(crate) mod task_assignment;
pub(crate) use spawn_plan_task::planned_task_title;
mod spawn_rollback;
mod spawn_shell;
pub(crate) mod spawn_title;
pub(crate) mod startup_lease;
pub(crate) mod startup_nudge;
mod status_reconciliation;
#[cfg(test)]
pub(crate) use status_reconciliation::try_snapshot_with_reconciled_status;
pub(crate) use status_reconciliation::SNAPSHOT_BUSY_ERROR;
pub(crate) mod restored_launch;
pub mod store;
pub(crate) mod stream_preview;
pub mod structured_prompt;
pub mod sync_watcher;
pub(crate) mod tab;
pub(crate) mod tab_adopt;
pub mod tab_budget;
pub mod tab_chat;
pub(crate) mod tab_lifecycle;
pub(crate) mod tab_repair;
pub(crate) mod tab_session_binding;
pub mod tab_transport;
pub mod tab_transport_chat;
mod team;
#[cfg(test)]
pub(crate) use team::close_team_with_state_change;
#[cfg(test)]
pub(crate) use team::rename_team_core;
#[cfg(test)]
pub(crate) use team::rename_team_with_state_change;
mod task_lane_guard;
pub(crate) use task_lane_guard::{
    canonical_path_of, classify_mutating_lane, live_canonical_writer, recorded_lane_matches,
    UnsafeMutatingLane,
};
// The value-only verdict has no production caller; the routes go through the
// ServiceContext wrapper instead.
#[cfg(test)]
pub(crate) use task_lane_guard::refuse_unsafe_task_bound_mutating_lane;
#[cfg(test)]
pub(crate) mod test_fixture;
#[cfg(test)]
mod tests;
pub mod tokens;
pub mod transport;
pub mod watcher;
pub mod web_access;
pub(crate) mod worker_ack;
mod worker_identity;
pub(crate) mod worker_interruption;
pub(crate) mod worker_lane;
#[cfg(test)]
pub(crate) use worker_lane::provision_lane_for_test;
pub(crate) use worker_lane::{
    is_linked_worktree, provision_isolated_lane, same_path, verify_worker_lane, LaneProtection,
    ProvisionedWorkerLane, WorkerLaneProvenance,
};
mod worker_persona;
mod workspace;
mod workspace_provision;

pub(crate) const AGENT_TAB_CLOSE_REFUSAL_PREFIX: &str = "workspace close refused";
pub(crate) const TEAM_CLOSE_REFUSAL_PREFIX: &str = "[TEAM CLOSE REFUSED]";
pub(crate) const WORKSPACE_CLOSE_CLEANUP_FAILURE_PREFIX: &str = "[WORKSPACE CLOSE CLEANUP FAILED]";

use std::fmt;
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
pub(crate) use agent_transition::{AgentPlacement, AgentTransitionRequest};
pub use hierarchy::TabStatusInfo;
pub(crate) use hierarchy::WorkerTurnSignal;
pub(crate) use hierarchy::{TeamPlanCaller, TeamPlanCreationError};
pub use inject_verify::{
    screen_contains_echo, snapshot_prompt_line, wait_for_submit_shell, InjectionError,
    InjectionResult, VerifyOpts,
};
pub use orchestration_pair::{OrchestrationPairParams, OrchestrationPairResult};
#[cfg(test)]
pub(crate) use orchestrator::{
    arm_provider_session_for_test, launch_waits_for_first_turn_binding, spawn_onboarding_task,
    LaunchGeneration,
};
pub(crate) use orchestrator::{
    move_chatbackend_task_to_system_prompt, record_provider_session, ProviderSessionBinding,
    TaskLaunchAuthority,
};
pub use orchestrator::{
    BatchOp, RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning, SpawnAgentParams,
    SpawnIdentityProvenance, SpawnResult,
};
pub(crate) use plan_authority::{
    ensure_planner_task_fields_are_structural, launch_binding_is_pending,
    planner_forbidden_task_field, AuthenticatedPlanCaller, OrchestratorLearningError,
    PlanAuthorityError,
};
pub(crate) use session_binding_state::SessionBindingState;
pub(crate) use spawn_params::starter_prompt_for_role;
pub use tab::{normalize_tab_description, MAX_TAB_DESCRIPTION_CHARS};
pub use tab_budget::{evaluate_tab_budget, TabBudgetLevel, TabBudgetNotice};
pub(crate) use team::TeamMutationCaller;
pub(crate) use team::TeamPlanError;
pub(crate) use team::{AddTeamMemberParams, AddTeamMemberResult};
pub(crate) use team::{TeamCartographerAttachLocks, TeamOrchestratorAttachLocks};
pub(crate) use worker_identity::{
    persist_attempt_executor_identity, persist_attempt_executor_identity_for_mux,
    respawn_title_provenance,
};
pub(crate) use worker_persona::WorkerPersonaReservations;

/// Shared service context — all state needed for multiplexer operations.
/// Both API routes, Tauri commands, and Socket server hold a clone of this.
#[derive(Clone)]
pub struct ServiceContext {
    pub(crate) agent_pad_data_dir: std::path::PathBuf,
    pub mux: MuxState,
    pub pty: PtyState,
    pub output_buf: OutputBufferState,
    pub screen_state: ScreenStateHandle,
    pub notif: NotifState,
    pub events: Arc<EventBroadcaster>,
    pub app_handle: tauri::AppHandle,
    pub(crate) cartographer_attach_locks: TeamCartographerAttachLocks,
    pub(crate) orchestrator_attach_locks: TeamOrchestratorAttachLocks,
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
    pub(crate) provider_tree_witnesses: Arc<process_tree::ProviderTreeWitnesses>,
    // Stable tab→pty map for hidden agent runs; survives close_tab so the unified AgentRun log endpoints can read closed-worker output.
    pub agent_run_pty_map: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
    // codeterm cron complete / mem worker report itself.
    pub run_watcher: Arc<crate::agent::run_watcher::RunWatcherRegistry>,
    /// Orchestrator-side view of remote (cross-mesh) workers, populated from
    /// inbound mesh reports. Drives the "remote Tab" ghost nodes in the UI.
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
    /// Per-tab Telegram topic bindings, sender identity and open authority.
    pub bridge_threads: Arc<codeterm_bridge::bridge::threads::ThreadService>,
    /// Per-tab last-restart timestamp for `agent_restart` rate limiting.
    pub restart_tracker: agent_restart::RestartTracker,
    /// Per-tab JSONL chat tailers — populated when a tab enters UI mode.
    pub chat_registry: Arc<crate::service::tab_chat::ChatRegistry>,
    pub delayed_send: crate::delayed_send::DelayedSendQueue,
    /// Active secret-store backend. Built-ins via factory; plugin backends
    /// (e.g. Bitwarden) come from `secret_registry`.
    pub secrets: crate::secrets::SecretStoreState,
    /// Registry of plugin-contributed secret backends, keyed by id.
    pub secret_registry: Arc<crate::secrets::SecretRegistry>,
    /// Registry of transcribers (built-in HTTP engine + any Transcriber plugin).
    /// Voice notes resolve the active transcriber here by id.
    pub transcribe_registry: Arc<crate::transcribe::TranscribeRegistry>,
    /// Registry of glance-view providers (plugins with `glanceView`). Plugin
    /// bubbles render their popovers from here.
    pub glance_registry: crate::glance::GlanceRegistryHandle,
    /// Registry of status-bubble providers (plugins with `statusBubble`). The
    /// tab-footer bubble row polls these per tab context.
    pub bubble_registry: crate::bubble::BubbleRegistryHandle,
    /// Sole owner of repository file watches. Status bubbles subscribe here
    /// per visible tab; the registry refcounts one watcher per identity.
    pub repo_watch: crate::repowatch::RepoWatchRegistryHandle,
    /// Registry of view providers (plugins with `view`). The iframe bridge
    /// (`ct.invoke`) routes here, gated by the plugin's permissions.
    pub view_registry: crate::view_host::ViewRegistryHandle,
    /// Registry of chatBackend providers (plugins with `chatBackend`). Non-PTY
    /// tabs route their I/O here via the `TabTransport` seam.
    pub chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle,
    /// Per-tab locks serializing the chatBackend restore→reopen transition so a
    /// concurrent send/deliver can't double-open one tab's plugin session.
    pub chatbackend_reopen_guards: Arc<tab_transport_chat::TabReopenGuards>,
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
    /// Registry of synchronous webhook receivers declared by JS plugins.
    /// Inbound API routes resolve the receiver here by manifest plugin id.
    pub webhook_registry: codeterm_plugin::WebhookRegistryHandle,
    /// Manifest-declared tab menu items, keyed by plugin id.
    pub tab_menu_registry: codeterm_plugin::tab_menu::TabMenuRegistryHandle,
    /// Sole owner of the webhook-receiver → tab bindings.
    pub receiver_targets: Arc<receiver_target::ReceiverTargetStore>,
    /// Coalesces `mux-state-changed` emits — see `emit_state_changed`.
    pub mux_emit_pending: Arc<AtomicBool>,
    /// Evidence counters for the daemon→desktop event path (event-freeze probe).
    pub mux_emit_stats: Arc<MuxEmitStats>,
    pub(crate) output_stream_activity:
        tokio::sync::broadcast::Sender<output_stream_activity::OutputStreamActivity>,
    /// Coalesces debounced session snapshots — see `schedule_session_persist_debounced`.
    pub session_persist_pending: Arc<AtomicBool>,
    pub services_store: Arc<crate::service::store::ServicesStore>,
    pub tokens: Arc<crate::service::tokens::TokenStore>,
    /// Ephemeral cross-mesh pastebin. In-memory only; daemon restart wipes
    /// all pastes. 20-entry cap, oldest-first eviction, TTL≤24h. (#98)
    pub pasteboard: crate::pasteboard::PasteboardState,
    /// Watches bytes about to be written to PTYs; a typed provider launch may
    /// claim a bare shell, but never contradict a claimed session.
    pub input_parser: Arc<input_parser::InputParser>,
    pub usage_store: crate::usage::UsageStoreHandle,
    pub usage_activity: crate::usage::TabActivity,
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
    pub(crate) deferred_session_bindings:
        Arc<session_binding_deferral::DeferredSessionBindingRegistry>,
    pub(crate) worker_identity_allocator:
        Arc<parking_lot::Mutex<worker_persona::WorkerPersonaReservations>>,
    pub(crate) worker_ack: Arc<worker_ack::WorkerAckRegistry>,
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
        tab_id: &str,
        channel: crate::multiplexer::types::PendingDeliveryChannel,
        payload: &str,
    ) -> Result<String, pending_delivery_payloads::PayloadStoreError> {
        self.create_pending_delivery_with_send_id(tab_id, channel, payload, None)
    }

    pub(crate) fn create_pending_delivery_with_send_id(
        &self,
        tab_id: &str,
        channel: crate::multiplexer::types::PendingDeliveryChannel,
        payload: &str,
        send_id: Option<&str>,
    ) -> Result<String, pending_delivery_payloads::PayloadStoreError> {
        use sha2::Digest;
        let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));
        let previous_digest = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .map(|record| record.content_digest.clone());
        self.pending_delivery_payloads
            .replace(previous_digest.as_deref(), &digest, payload)?;
        let mut mux = self.mux.write();
        let tab = mux.get_tab_mut(tab_id).ok_or_else(|| {
            pending_delivery_payloads::PayloadStoreError::Io {
                digest: digest.clone(),
                message: format!("tab {tab_id} vanished while creating pending delivery"),
            }
        })?;
        let record = crate::multiplexer::types::PendingDeliveryRecord::new(digest.clone(), channel);
        tab.pending_delivery = Some(match send_id {
            Some(send_id) => record.with_send_id(send_id.to_string()),
            None => record,
        });
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
        tab_id: &str,
    ) -> Result<Option<String>, pending_delivery_payloads::PayloadStoreError> {
        use crate::multiplexer::types::PendingDeliveryChannel;
        let pending = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.clone());
        match pending {
            Some(record)
                if record.state().needs_delivery()
                    && record.channel == PendingDeliveryChannel::InteractiveInput =>
            {
                self.pending_delivery_payloads
                    .resolve(&record.content_digest)
                    .map(Some)
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn reconcile_pending_delivery(
        &self,
        tab_id: &str,
        evidence: Result<bool, delivery_error::DeliveryError>,
    ) -> Result<(), delivery_error::DeliveryError> {
        use crate::multiplexer::types::PendingDeliveryChannel;
        let has_pending_delivery = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .filter(|record| record.channel == PendingDeliveryChannel::InteractiveInput)
            .is_some();
        if !has_pending_delivery {
            return evidence.and_then(|observed| {
                if observed {
                    Ok(())
                } else {
                    Err(delivery_error::DeliveryError::SubmitUnconfirmed {
                        tab: tab_id.to_string(),
                        detail: "delivery acknowledgement not observed".into(),
                    })
                }
            });
        }
        let mut mux = self.mux.write();
        let tab = mux.get_tab_mut(tab_id).ok_or_else(|| {
            delivery_error::DeliveryError::SubmitUnconfirmed {
                tab: tab_id.to_string(),
                detail: "tab vanished while reconciling pending delivery".into(),
            }
        })?;
        let reconciliation = tab.pending_delivery.as_mut().map_or(Ok(()), |record| {
            crate::service::launch_delivery::reconcile_pending_delivery(tab_id, record, evidence)
        });
        drop(mux);
        crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
        .map_err(|message| delivery_error::DeliveryError::SubmitUnconfirmed {
            tab: tab_id.to_string(),
            detail: format!("pending delivery persistence failed: {message}"),
        })?;
        reconciliation?;
        self.cleanup_confirmed_pending_delivery(tab_id)
            .map_err(|error| delivery_error::DeliveryError::SubmitUnconfirmed {
                tab: tab_id.to_string(),
                detail: format!("confirmed payload cleanup failed: {error}"),
            })?;
        Ok(())
    }

    pub(crate) fn confirm_pending_delivery(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Result<(), pending_delivery_payloads::PayloadStoreError> {
        self.cleanup_pending_delivery(tab_id, true, "confirmed", Some(attempt_id))
    }

    pub(crate) fn cleanup_confirmed_pending_delivery(
        &self,
        tab_id: &str,
    ) -> Result<(), pending_delivery_payloads::PayloadStoreError> {
        self.cleanup_pending_delivery(tab_id, true, "confirmed", None)
    }

    fn discard_pending_delivery(
        &self,
        tab_id: &str,
    ) -> Result<(), pending_delivery_payloads::PayloadStoreError> {
        self.cleanup_pending_delivery(tab_id, false, "explicit_discard", None)
    }

    fn cleanup_pending_delivery(
        &self,
        tab_id: &str,
        accepted_only: bool,
        decision: &str,
        expected_attempt_id: Option<&str>,
    ) -> Result<(), pending_delivery_payloads::PayloadStoreError> {
        let record = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return Ok(());
            };
            if accepted_only
                && !tab
                    .pending_delivery
                    .as_ref()
                    .is_some_and(|record| record.state().is_accepted())
            {
                return Ok(());
            }
            if expected_attempt_id.is_some_and(|expected| {
                !tab.pending_delivery.as_ref().is_some_and(|record| {
                    record
                        .attempt_provenance()
                        .is_some_and(|attempt| attempt.attempt_id == expected)
                })
            }) {
                return Ok(());
            }
            tab.pending_delivery.take()
        };
        let Some(record) = record else { return Ok(()) };
        let digest = record.content_digest.clone();
        let mut retired = Some(record);
        if let Err(message) = crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        ) {
            if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
                tab.pending_delivery = retired.take();
            }
            return Err(pending_delivery_payloads::PayloadStoreError::Io { digest, message });
        }
        if let Err(error) = self
            .pending_delivery_payloads
            .free_terminal_pair(&mut retired, decision)
        {
            if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
                tab.pending_delivery = retired.take();
            }
            let rollback = crate::service::session_persist::persist_session_after_mutation(
                self,
                crate::service::session_persist::SessionPersistUrgency::Prompt,
            );
            return Err(pending_delivery_payloads::PayloadStoreError::Io {
                digest,
                message: match rollback {
                    Ok(()) => error.to_string(),
                    Err(message) => format!("{error}; record rollback failed: {message}"),
                },
            });
        }
        Ok(())
    }
    // ── Event helpers (used by all sub-modules) ─────────────────────

    /// Notify the UI that an orchestrator's remote (cross-mesh) workers changed.
    /// One payload shape for every mutation site (spawn registration, UI relay
    /// hook, inbound report) so the sidebar refreshes consistently.
    pub(crate) fn emit_remote_workers_changed(&self, orchestrator_tab: &str) {
        // SSE bus, so the web mirror updates live too — not only on next remount.
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "remote-workers-changed",
            serde_json::json!({
                "orchestrator_tab_id": orchestrator_tab,
                "workers": self.remote_workers.snapshot(),
            }),
        );
    }

    pub(crate) fn emit_state_changed(&self) {
        publish_mux_state_changed(&self.events);
        self.emit_state_changed_after_bus_publish();
    }

    pub(crate) fn emit_state_changed_after_bus_publish(&self) {
        self.output_stream_mux_changed();
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
        self.output_stream_mux_changed();
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
    /// ONLY path that keeps a browser tab's `tabStatuses` fresh between page
    /// loads. Without it, `tab_statuses` here is always empty and the
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

    pub(crate) fn emit_tab_title_pinned(&self, tab_id: &str, title: &str) {
        self.events
            .publish(crate::api::events::SystemEvent::TabTitlePinned {
                tab_id: tab_id.to_string(),
                title: title.to_string(),
            });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"tab_id": tab_id, "title": title});
        std::thread::spawn(move || {
            let _ = handle.emit("tab-title-pinned", payload);
        });
    }

    /// Emit a focus-change event to the frontend. Delayed 75 ms so it always
    /// arrives AFTER the coalesced 50 ms `mux-state-changed` window — this way
    /// the frontend's structural state is up to date before it tries to apply
    /// focus to (potentially newly created) tabs. Frontend listener also has a
    /// short retry as defense-in-depth.
    pub(crate) fn emit_tab_focus_changed(&self, tab_id: &str) {
        self.events
            .publish(crate::api::events::SystemEvent::TabFocused {
                tab_id: tab_id.to_string(),
            });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"tab_id": tab_id});
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            let _ = handle.emit("tab-focused", payload);
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
            let live_team_ids = self.live_team_ids();
            self.reconcile_dangling_team_plan_bindings(
                &live_team_ids,
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
        AgentRole::from_str_compat(role_str)
            .ok_or_else(|| format!("Invalid role: {role_str}. Use: Orchestrator, Manager, Worker"))
    }

    /// The one place wire role sugar resolves: legacy `Orchestrator`/`Conductor`
    /// literals name the orchestrator kind. Core code branches on the kind.
    pub(crate) fn parse_role_kind(
        role_str: &str,
        role_profile: Option<&str>,
    ) -> Result<(AgentRole, Option<String>), String> {
        let role = Self::parse_role(role_str)?;
        let profile = match role_str {
            "Orchestrator" | "Conductor" => {
                Some(codeterm_types::agent::role_kind::builtins::ORCHESTRATOR.to_string())
            }
            _ => role_profile
                .map(str::trim)
                .filter(|profile| !profile.is_empty())
                .map(str::to_string),
        };
        if role == AgentRole::Manager && profile.is_none() {
            return Err("Manager requires a named role_profile".into());
        }
        Ok((role, profile))
    }
}

pub(crate) fn publish_mux_state_changed(events: &crate::api::events::EventBroadcaster) {
    events.publish(crate::api::events::SystemEvent::MuxStateChanged);
}

// ── Shared data types ───────────────────────────────────────────────

#[derive(Debug)]
pub struct ReportSubmissionError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[cfg(test)]
impl ReportSubmissionError {
    pub(crate) fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}

impl fmt::Display for ReportSubmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ReportSubmissionError {}

impl From<String> for ReportSubmissionError {
    fn from(message: String) -> Self {
        Self {
            code: crate::service::plan::RPC_INTERNAL_ERROR,
            message,
            data: None,
        }
    }
}

impl From<crate::service::plan::PlanError> for ReportSubmissionError {
    fn from(error: crate::service::plan::PlanError) -> Self {
        Self {
            code: error.rpc_code(),
            message: error.to_string(),
            data: Some(error.to_json()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportResult {
    pub report_id: String,
    pub orchestrator_tab_id: String,
    /// Non-fatal notices from plan/task resolution (stale env ids, auto-resolution).
    pub notices: Vec<String>,
    pub persisted_task: Option<(String, String)>,
    pub persisted_plan_evidence: Option<(String, String)>,
    pub rejected_task_transition: Option<RejectedTaskTransition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RejectedTaskTransition {
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub cause: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_outcome: Option<codeterm_types::agent::ReportOutcomeClass>,
}

/// Helper: extract optional string from JSON value.
pub(crate) fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Replace "$ref" strings in params with actual tab_ids from previous results.
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
