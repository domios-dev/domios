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

mod construct;
pub use construct::build_service_context;
pub mod account;
pub mod activity;
pub(crate) mod agent_delivery_outbox;
pub(crate) mod agent_fork;
pub mod agent_pad;
pub mod agent_restart;
pub mod agent_runtime;
pub mod agent_transition;
pub(crate) mod ai_helper_spawn;
pub mod lazy_restore;
pub use ai_helper_spawn::AiHelperRequest;
pub mod analytics;
mod analytics_host;
pub(crate) mod background_work;
pub(crate) mod background_work_watch;
pub mod billing;
pub(crate) mod boot_owner_prompt;
pub mod boot_stage;
pub(crate) mod boot_wait;
pub(crate) mod bridge_threads;
pub use codeterm_browser as browser;
pub(crate) mod capability_probe;
pub use capability_probe::refresh as capability_probe_refresh;
pub(crate) mod cartographer;
pub(crate) mod chat_send_evidence;
pub mod chat_send_transaction;
mod close_history;
pub mod codex_rpc;
mod command_port_impl;
pub mod context_feed;
pub mod cron;
pub mod delivery_error;
pub mod general_agent;
pub mod health;
pub mod hierarchy;
pub mod hook_session_binding;
pub mod inject_verify;
pub mod input_parser;
pub mod input_transport;
pub mod instance_env;
pub mod integration_silence;
pub mod kpi_hooks;
pub mod lane_removal;
pub mod lane_stats;
pub mod launch_artifacts;
pub mod launch_delivery;
pub use launch_delivery::prepare_spawn_input as launch_delivery_prepare_spawn_input;
pub(crate) mod launch_diagnostics;
pub(crate) mod launch_input_gate;
pub mod launch_readiness;
pub mod launch_session_detection;
pub mod local_preview;
pub(crate) mod mesh_artifacts;
pub use mesh_artifacts::transfer as mesh_artifacts_transfer;
pub mod mesh_attachment;
pub mod message_gateway;
pub mod messenger;
pub(crate) mod model_defaults;
pub mod model_providers;
pub mod notification;
mod notification_desktop;
pub(crate) mod notification_push;
pub(crate) mod open_url;
pub use open_url::open_url as open_url_open_url;
mod paired_planner;
pub use paired_planner::{bind_pending_paired_planner, tab_has_pending_paired_planner};
pub(crate) mod orchestrator;
pub mod ordered_actions;
pub mod parked_user_delivery;
pub mod post_update;
pub(crate) mod process_tree;
pub mod prompt_resolution;
#[cfg(any(test, feature = "test-support"))]
pub use prompt_resolution::tests as prompt_resolution_tests;
pub(crate) mod protocol_recorder;
pub use protocol_recorder::ProtocolRecorder;
pub mod provider_cli;
mod published_mux_state;
pub use provider_cli::provider_cli_status as provider_cli_provider_cli_status;
use published_mux_state::PublishedMuxState;
pub mod provider_cli_actions;
pub mod provider_hooks;
pub(crate) mod provider_signals;
pub use provider_signals::apply_structured as provider_signals_apply_structured;
mod provider_signals_probe;
pub(crate) mod provider_update;
pub(crate) mod pty_resize;
pub use pty_resize::resize as pty_resize_resize;
pub(crate) mod public_send_readiness;
pub(crate) mod skill_mirror;
pub(crate) mod startup_turn;
pub(crate) mod task_files;
pub use orchestrator::build_agent_command_for_service;
pub(crate) use orchestrator::provider_launch_task;
#[cfg(test)]
pub(crate) use orchestrator::role_onboarding_task;
pub use orchestrator::tab_relink_signal::{await_relink, await_relink_async};
mod directory_pre_trust;
pub(crate) mod directory_trust;
mod folder_trust_carryover;
pub mod orchestrator_digests;
mod output;
mod output_feed;
pub mod output_stream_activity;
pub mod pending_delivery_payloads;
pub(crate) mod permission_auto_confirm;
pub mod permission_decision;
pub mod permission_policy;
pub use permission_policy::configured_permission_bypass_for as permission_policy_configured_permission_bypass_for;
pub mod persona;
pub mod plan;
pub mod plan_authority;
pub mod plan_batch_authority;
mod plan_pause;
pub mod plan_reconcile;
mod prompt_chain;
mod provider_handoff;
use codeterm_util::pty_frame_timing;
mod pty_output_pipeline;
pub use plan_reconcile::tab_has_live_agent;
pub(crate) mod local_recording;
pub mod managed_tabs;
pub mod plugin_host;
pub mod plugin_prompt;
pub mod plugin_skills;
pub mod provider_identity;
pub(crate) mod pty_activation;
pub mod pty_io;
pub mod pty_lifecycle;
pub(crate) mod pty_ownership;
pub mod push;
mod reasoning_effort;
pub mod receiver_target;
pub use receiver_target::ReceiverTargetError;
pub mod remote_tabs;
pub mod remote_workers;
pub mod reply_relay;
pub(crate) mod restore_exited_member;
mod restored_binding;
pub mod role_identity;
pub mod role_preset;
pub mod role_profile;
mod role_target;
mod session_backup;
pub mod session_binding_deferral;
mod session_binding_state;
pub(crate) mod session_model;
pub mod session_persist;
pub(crate) mod shell_exec;
pub use shell_exec::ExecWaiter;
mod shell_restart;
pub mod smart_add;
pub mod spawn;
pub mod spawn_capability;
mod spawn_chatbackend;
mod spawn_cli;
pub mod spawn_cwd;
pub use spawn_cwd::CwdRefusal;
pub mod spawn_factory;
mod spawn_hidden_launch;
pub mod spawn_launch_verdict;
pub mod spawn_params;
pub mod spawn_plan_task;
pub mod task_assignment;
pub mod unattended_launch;
pub use spawn_plan_task::planned_task_title;
mod spawn_rollback;
mod spawn_shell;
pub(crate) mod spawn_title;
pub use spawn_title::validate_agent_tab_title as spawn_title_validate_agent_tab_title;
pub mod startup_lease;
pub(crate) mod startup_nudge;
mod status_reconciliation;
pub(crate) mod stop_execution;
pub use stop_execution::stop_agent_turn as stop_execution_stop_agent_turn;
mod tab_status_events;
pub(crate) mod tab_stop;
pub mod turn_resume;
pub(crate) mod utility_deadline;
pub(crate) mod utility_deadline_model;
pub use utility_deadline_model::parse_timeout as utility_deadline_model_parse_timeout;
mod utility_tabs;
pub use status_reconciliation::SNAPSHOT_BUSY_ERROR;
pub mod blocking;
pub use blocking::run as blocking_run;
pub(crate) mod restored_launch;
pub mod store;
pub(crate) mod stream_preview;
pub(crate) mod structured_endpoint;
pub mod structured_prompt;
pub mod supporter;
pub(crate) mod supporter_notify;
pub mod supporter_tab_state;
pub mod surface;
pub mod sync_watcher;
pub mod tab;
pub mod tab_adopt;
pub mod tab_budget;
pub mod tab_chat;
pub(crate) mod tab_conditions;
pub(crate) mod tab_lifecycle;
pub mod tab_repair;
pub(crate) mod tab_session_binding;
pub mod tab_transport;
pub mod tab_transport_chat;
pub mod task_lane_guard;
pub mod team_core;
pub mod team_membership;
pub use task_lane_guard::{
    canonical_path_of, classify_mutating_lane, live_canonical_writer, recorded_lane_matches,
    UnsafeMutatingLane,
};
// The value-only verdict has no production caller; the routes go through the
// ServiceContext wrapper instead.
#[cfg(any(test, feature = "test-support"))]
pub use task_lane_guard::refuse_unsafe_task_bound_mutating_lane;
#[cfg(test)]
mod persistence_roundtrip_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod test_fixture;
#[cfg(test)]
mod tests;
pub mod tokens;
pub mod transport;
pub mod watcher;
pub mod web_access;
#[cfg(windows)]
pub use codeterm_util::windows_job;
pub mod worker_ack;
pub mod worker_identity;
pub(crate) mod worker_interruption;
pub mod worker_lane;
pub(crate) mod worker_loss;
#[cfg(any(test, feature = "test-support"))]
pub use worker_lane::provision_lane_for_test;
pub use worker_lane::{
    is_linked_worktree, provision_isolated_lane, same_path, verify_worker_lane, LaneProtection,
    ProvisionedWorkerLane, WorkerLaneProvenance,
};
pub use worker_loss::BUSY_WORKER_CLOSE_REFUSAL_PREFIX;
mod worker_persona;
mod workspace;
pub(crate) mod workspace_cwd;
mod workspace_provision;

pub const AGENT_TAB_CLOSE_REFUSAL_PREFIX: &str = "workspace close refused";
pub const TEAM_CLOSE_REFUSAL_PREFIX: &str = "[TEAM CLOSE REFUSED]";
pub const TEAM_CLOSE_BUSY_REFUSAL_PREFIX: &str = "[TEAM CLOSE REFUSED] busy";
pub const WORKSPACE_CLOSE_CLEANUP_FAILURE_PREFIX: &str = "[WORKSPACE CLOSE CLEANUP FAILED]";

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::agent::registry::AgentRegistry;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::AgentRole;
use crate::{
    AgentTrackerState, NotifState, OutputBufferState, PromptDetectorState, PtyState,
    ReportStoreState, ScreenStateHandle,
};
use codeterm_events::EventBroadcaster;

// Re-export public types used by callers.
pub use agent_transition::AgentPlacement;
pub use agent_transition::AgentTransitionRequest;
#[cfg(any(test, feature = "test-support"))]
pub use agent_transition::{execute_transition_at, TransitionFailurePoint};
pub use hierarchy::TabStatusInfo;
pub use hierarchy::{TeamPlanCaller, TeamPlanCreationError, WorkerTurnSignal};
pub use inject_verify::{
    screen_contains_echo, snapshot_prompt_line, wait_for_submit_shell, InjectionError,
    InjectionResult, VerifyOpts,
};
#[cfg(any(test, feature = "test-support"))]
pub use orchestrator::arm_provider_session_for_test;
#[cfg(any(test, feature = "test-support"))]
pub use orchestrator::launch_waits_for_first_turn_binding;
#[cfg(any(test, feature = "test-support"))]
pub use orchestrator::spawn_onboarding_task;
pub use orchestrator::{
    move_chatbackend_task_to_system_prompt, record_provider_session, LaunchGeneration,
    ProviderSessionBinding, RemoteTaskBinding, TaskLaunchAuthority,
};
pub use orchestrator::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};
pub use plan_authority::{
    ensure_planner_task_fields_are_structural, launch_binding_is_pending,
    planner_forbidden_task_field, AuthenticatedPlanCaller, OrchestratorLearningError,
    PlanAuthorityError,
};
pub use session_binding_state::SessionBindingState;
pub use spawn_params::starter_prompt_for_role;
pub use tab::{normalize_tab_description, MAX_TAB_DESCRIPTION_CHARS};
pub use tab_budget::{evaluate_tab_budget, TabBudgetLevel, TabBudgetNotice};
pub use team_core::TeamMutationCaller;
pub use team_core::TeamPlanError;
pub use worker_identity::{
    persist_attempt_executor_identity, persist_attempt_executor_identity_for_mux,
    respawn_title_provenance, restore_attempt_binding_for_caller, restore_attempt_binding_for_mux,
    restore_review_attempt_binding_for_mux, AttemptBindingRestoreResult,
    AttemptBindingRestoreTarget,
};
pub(crate) use worker_persona::{WorkerIdentityAllocator, WorkerPersonaReservations};

mod configuration;
pub(crate) mod extensions;
pub(crate) use extensions::Extensions;
pub use extensions::ExtensionsBuilder;

/// Shared service context — all state needed for multiplexer operations.
/// Both API routes, Tauri commands, and Socket server hold a clone of this.
#[derive(Clone)]
pub struct ServiceContext {
    pub lifecycle_journal: Arc<crate::session::lifecycle::TabLifecycleJournal>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) ready_user_send_tasks: Arc<test_fixture::TestReadySendTasks>,
    pub(crate) agent_pad_data_dir: std::path::PathBuf,
    pub mux: MuxState,
    pub pty: PtyState,
    pub output_buf: OutputBufferState,
    pub screen_state: ScreenStateHandle,
    pub notif: NotifState,
    pub events: Arc<EventBroadcaster>,
    pub app_handle: crate::app_runtime::AppHandle,
    pub ui: Arc<dyn crate::ports::host::UiEvents>,
    pub shell: Arc<dyn crate::ports::host::Shell>,
    pub rt: tokio::runtime::Handle,
    pub agent_tracker: AgentTrackerState,
    pub report_store: ReportStoreState,
    /// Orchestration-economics event writer. Disabled (and file-less) unless `kpi.enabled`.
    pub kpi: crate::agent::kpi::KpiRecorder,
    pub agent_registry: Arc<AgentRegistry>,
    pub mem: Arc<crate::mem::MemManager>,
    pub activity: Arc<crate::mem::activity::ActivityRing>,
    pub workers: Arc<crate::mem::workers::GraphWorkerRegistry>,
    pub prompt_detector: PromptDetectorState,
    pub pty_ownership: pty_ownership::PtyOwnership,
    // Stable tab→pty map for hidden agent runs; survives close_tab so the unified AgentRun log endpoints can read closed-worker output.
    pub agent_run_pty_map: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
    // codeterm cron complete / mem worker report itself.
    pub run_watcher: Arc<crate::agent::run_watcher::RunWatcherRegistry>,
    /// Orchestrator-side view of remote (cross-mesh) workers, populated from
    /// inbound mesh reports. Drives the "remote Tab" ghost nodes in the UI.
    pub remote_workers: crate::service::remote_workers::RemoteWorkerRegistryHandle,
    /// Channel for outbound Telegram messages (CLI bridge.send command).
    pub bridge_outbound: crate::bridge::manager::OutboundSender,
    /// Per-tab JSONL chat tailers — populated when a tab enters UI mode.
    pub chat_registry: Arc<crate::service::tab_chat::ChatRegistry>,
    /// Active secret-store backend. Built-ins via factory; plugin backends
    /// (e.g. Bitwarden) come from `secret_registry`.
    pub secrets: crate::secrets::SecretStoreState,
    /// Decision adapters for the stage-2 recall re-rank, resolved by configured id.
    pub supporter_models: Arc<crate::service::supporter::registry::SupporterRegistry>,
    /// Registry of chatBackend providers (plugins with `chatBackend`). Non-PTY
    /// tabs route their I/O here via the `TabTransport` seam.
    pub chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle,
    /// Unified facade over the agent + chat-backend stores. Listing and
    /// spawn-classify go through this so call-sites stop touching two registries.
    pub provider_registry: Arc<crate::provider::ProviderRegistry>,
    pub local_recordings: Arc<local_recording::RecordingLeases>,
    pub services_store: Arc<crate::service::store::ServicesStore>,
    pub tokens: Arc<crate::service::tokens::TokenStore>,
    pub usage_store: crate::usage::UsageStoreHandle,
    /// Global plan store with project-scoped queries. Sole writer.
    pub plan_store: Arc<crate::service::plan::PlanStore>,
    pub plan_turn_counter: Arc<crate::service::plan::reminders::TurnCounter>,
    pub pending_delivery_payloads: Arc<pending_delivery_payloads::PendingDeliveryPayloadStore>,
    pub chat_send_ledger: Arc<chat_send_transaction::ChatSendLedger>,
    pub deferred_session_bindings: Arc<session_binding_deferral::DeferredSessionBindingRegistry>,
    pub worker_ack: Arc<worker_ack::WorkerAckRegistry>,
    /// The single optimistic outbound-delivery seam with idempotency.
    /// Callers migrate onto `gateway.submit(...)` incrementally (task_1iiwnc).
    pub gateway: Arc<crate::service::message_gateway::MessageGateway>,
    pub(crate) extensions: Arc<Extensions>,
}

pub type ServiceState = Arc<ServiceContext>;

impl ServiceContext {
    pub fn ext<T: std::any::Any + Send + Sync>(&self) -> &T {
        self.extensions.get::<T>()
    }
}

/// Coalesces `mux-state-changed` emits — see `emit_state_changed`.
pub struct MuxEmitPending(pub Arc<AtomicBool>);

pub(crate) struct PublishedTabStatuses(
    pub(crate) Arc<parking_lot::Mutex<HashMap<String, crate::agent::status::TabStatus>>>,
);

pub struct PlanWatchers(
    pub  Arc<
        parking_lot::Mutex<HashMap<std::path::PathBuf, crate::service::plan::watcher::PlanWatcher>>,
    >,
);

pub struct LoadedPlanProjects(
    pub Arc<parking_lot::Mutex<std::collections::HashSet<std::path::PathBuf>>>,
);

pub struct PlanStartupLoaded(pub Arc<AtomicBool>);

/// Receiver half — taken when bridge starts. Replaced on restart with a new channel.
pub struct BridgeOutboundRx(
    pub  Arc<
        parking_lot::Mutex<
            Option<tokio::sync::mpsc::UnboundedReceiver<crate::bridge::manager::OutboundMessage>>,
        >,
    >,
);

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

pub fn start_plan_reminder_scheduler(svc: ServiceState) {
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
    pub fn emit_remote_workers_changed(&self, orchestrator_tab: &str) {
        // SSE bus, so the web mirror updates live too — not only on next remount.
        crate::events::emit_everywhere(
            &self.app_handle,
            "remote-workers-changed",
            serde_json::json!({
                "orchestrator_tab_id": orchestrator_tab,
                "workers": self.remote_workers.snapshot(),
            }),
        );
    }

    pub fn emit_state_changed(&self) {
        publish_mux_state_changed(&self.events);
        self.emit_state_changed_after_bus_publish();
    }

    pub fn emit_state_changed_after_bus_publish(&self) {
        self.publish_authoritative_tab_statuses();
        self.output_stream_mux_changed();
        self.ext::<Arc<crate::service::MuxEmitStats>>()
            .requests
            .fetch_add(1, Ordering::Relaxed);
        #[cfg(debug_assertions)]
        if self
            .ext::<Arc<crate::service::MuxEmitStats>>()
            .frozen
            .load(Ordering::Acquire)
        {
            self.schedule_session_persist_debounced();
            self.ext::<Arc<crate::service::MuxEmitStats>>()
                .suppressed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        // Coalesce frontend emits to ≤1/50ms — output.rs fires this 4× per PTY chunk.
        if self
            .ext::<crate::service::MuxEmitPending>()
            .0
            .swap(true, Ordering::AcqRel)
        {
            self.ext::<Arc<crate::service::MuxEmitStats>>()
                .suppressed
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.ext::<Arc<crate::service::MuxEmitStats>>()
            .pending_since_ms
            .store(wall_now_ms(), Ordering::Relaxed);
        let handle = self.app_handle.clone();
        let pending = self.ext::<crate::service::MuxEmitPending>().0.clone();
        let mux = self.mux.clone();
        let agent_tracker = self.agent_tracker.clone();
        let agent_registry = self.agent_registry.clone();
        let stats = self.ext::<Arc<crate::service::MuxEmitStats>>().clone();
        let published = self.ext::<Arc<PublishedMuxState>>().clone();
        let events = self.events.clone();
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
                match published.publish(snapshot.generation, &snapshot) {
                    Ok(true) => {
                        events.publish_generic("mux-state-changed", serde_json::json!({}));
                    }
                    Ok(false) => {}
                    Err(error) => log::error!("[mux-snapshot] publication failed: {error}"),
                }
                if let Err(error) =
                    crate::service::session_persist::submit_session_snapshot_with_intent(
                        &handle,
                        &mux,
                        crate::session::store::SaveIntent::PreserveDesk,
                    )
                {
                    log::warn!("[session-persist] coalesced desk submission failed: {error}");
                }
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

    /// Socket mutations use the same published view and coalescing as local mutations.
    pub fn emit_mux_state_changed_empty(&self) {
        self.emit_state_changed();
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
            .publish(codeterm_events::SystemEvent::TabTitlePinned {
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
            .publish(codeterm_events::SystemEvent::TabFocused {
                tab_id: tab_id.to_string(),
            });
        let handle = self.app_handle.clone();
        let payload = serde_json::json!({"tab_id": tab_id});
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            let _ = handle.emit("tab-focused", payload);
        });
    }

    pub fn kill_ptys(&self, pty_ids: &[String]) {
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
            let loaded = self.ext::<crate::service::LoadedPlanProjects>().0.lock();
            !loaded.contains(&project_root)
        };
        if should_scan {
            self.plan_store.scan_project(&project_root)?;
            self.ext::<crate::service::LoadedPlanProjects>()
                .0
                .lock()
                .insert(project_root.clone());
            let live_team_ids = team_core::live_team_ids(self);
            team_core::reconcile_dangling_team_plan_bindings_and_emit(
                self,
                &live_team_ids,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )?;
        }
        let legacy_plans_dir = project_root.join(".codeterm").join("plans");
        let mut watchers = self.ext::<crate::service::PlanWatchers>().0.lock();
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
        self.ext::<crate::service::PlanStartupLoaded>()
            .0
            .load(Ordering::Acquire)
    }

    pub fn mark_plans_startup_loaded(&self) {
        self.ext::<crate::service::PlanStartupLoaded>()
            .0
            .store(true, Ordering::Release);
        self.notify_event("plans-loaded", serde_json::json!({}));
    }

    /// Unified event dispatch — sends to Tauri frontend and external webhooks.
    /// OS notifications are NOT sent here — use in-app toasts only.
    pub fn notify_event(&self, event_type: &str, payload: serde_json::Value) {
        // 1. Native webview + SSE bus (web mirror) through the single seam.
        crate::events::emit_everywhere(&self.app_handle, event_type, &payload);
        // 2. External webhooks + websockets (Telegram, Discord, etc.)
        self.events.broadcast(event_type, payload);
    }

    pub fn parse_role(role_str: &str) -> Result<AgentRole, String> {
        AgentRole::from_str_compat(role_str)
            .ok_or_else(|| format!("Invalid role: {role_str}. Use: Orchestrator, Manager, Worker"))
    }

    /// The one place wire role sugar resolves: legacy `Orchestrator`/`Conductor`
    /// literals name the orchestrator kind. Core code branches on the kind.
    pub fn parse_role_kind(
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

pub fn publish_mux_state_changed(events: &codeterm_events::EventBroadcaster) {
    events.publish(codeterm_events::SystemEvent::MuxStateChanged);
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
pub fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Replace "$ref" strings in params with actual tab_ids from previous results.
pub fn resolve_refs(
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

pub(crate) mod context_bundle;
pub(crate) mod tool_less_instructions;

#[cfg(any(test, feature = "test-support"))]
pub use plan_authority::test_fixture::fixture as plan_authority_fixture;

#[cfg(any(test, feature = "test-support"))]
pub use workspace::test_fixture::workspace_with_agent_tab as workspace_agent_fixture;

#[cfg(any(test, feature = "test-support"))]
pub use spawn::prepare_tab_identity_for_test;

#[cfg(any(test, feature = "test-support"))]
pub use plan_pause::{PlanPausePorts, ServicePlanPausePorts};

pub use worker_ack::STARTUP_BINDING_DEADLINE;

pub use background_work::incomplete_snapshot;
pub use local_recording::{CaptureClipDescriptor, LocalRecordingCommitOutcome, RecordingLeases};
pub use orchestrator::mark_tab_agent_launch_started;
pub use pty_ownership::{LinkOutcome, PtyOwnership, PtySelection, TabPtyClaim};

pub use background_work::apply as background_work_apply;
pub use launch_diagnostics::AgentLaunchOrigin;
#[cfg(any(test, feature = "test-support"))]
pub use launch_session_detection::registered_launch_marker;
pub use orchestrator::launch_agent_in_background_with_env;
pub use provider_signals::apply as provider_signals_apply;
pub use restored_launch::{
    persist_restored_launch_identity, restore_permission_bypass_authorization,
    restored_fresh_launch_input, restored_resume_session_id, restored_session_resumable,
    RestoredFreshLaunchInput,
};
#[cfg(any(test, feature = "test-support"))]
pub use skill_mirror::extract_revision;
#[cfg(any(test, feature = "test-support"))]
pub use structured_endpoint::remove as structured_endpoint_remove;
pub use structured_endpoint::sse as structured_endpoint_sse;

#[cfg(any(test, feature = "test-support"))]
pub use pty_ownership::OwnershipEvent;

pub use model_defaults::start as start_model_defaults;
pub use provider_update::check_available as check_provider_update_available;
pub use restore_exited_member::{apply_restore_decisions, mark_legacy_role_shell_failures};
pub use skill_mirror::{mirror_skills_to_providers, seed_bundled_skills};
pub use utility_deadline::restore as restore_utility_deadlines;

pub use supporter_notify::record_launch as supporter_record_launch;
