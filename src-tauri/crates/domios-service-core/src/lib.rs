pub mod agent;
pub mod agent_commands;
pub mod api_support;
pub mod async_runtime;
pub use codeterm_util::attachments;
pub mod bridge;
pub mod bridge_host;
pub mod bubble;
pub mod chatbackend;
pub mod chatpipeline;
pub mod chatpreprocessor;
pub mod config;
pub mod crash;
pub mod cron;
pub mod delayed_send;
pub mod dispatch;
mod fdlimit;
pub mod flow_trace;
pub mod frontend_bundle_api_version;
pub mod glance;
mod instance_lifetime;
pub mod keepawake;
mod liveness;
pub mod lock_hold;
pub mod logs;
pub mod mem;
pub mod mem_host;
pub mod mem_ops;
pub mod mesh;
pub mod mesh_host;
pub mod message_markers;
pub mod multiplexer;
pub mod notification;
pub mod pasteboard;
pub mod paths;
pub use codeterm_util::persisted_state;
pub mod plan;
pub mod plugin;
pub mod ports;
pub mod process_util;
pub mod prompts;
pub mod provider;
pub mod pty;
pub mod repowatch;
pub mod resize_owner;
pub mod runaway_sweeper;
pub mod secrets;
pub mod service;
pub mod session;
pub mod shell;
pub mod startup;
pub mod storage;
#[cfg(test)]
mod telegram269_wire_contract;
pub mod transcribe;
pub mod transcribe_adapter;
pub mod usage;
pub mod user_last_activity;
pub mod view;
pub mod view_host;
mod webview2_reap;

use std::sync::Arc;

use parking_lot::Mutex;

use multiplexer::commands::MuxState;
pub use notification::NotifState;
use pty::manager::{OutputBuffer, PtyManager};
use pty::screen_state::ScreenState;
use service::ServiceState;

pub type PtyState = Arc<lock_hold::TimedMutex<PtyManager>>;
pub type OutputBufferState = Arc<lock_hold::TimedMutex<OutputBuffer>>;
pub type ScreenStateHandle = Arc<lock_hold::TimedMutex<ScreenState>>;
pub type AgentTrackerState = Arc<lock_hold::TimedMutex<agent::status::TabStatusTracker>>;
pub type ShellCapabilityState = Arc<Mutex<agent::command_detection::ShellCapabilityTracker>>;
pub type ReportStoreState = Arc<Mutex<agent::reports::ReportStore>>;
pub type PromptDetectorState = Arc<Mutex<agent::prompt::PromptDetector>>;
pub use cron::scheduler::{CronState, CronWake};
pub type MeshStateHandle = mesh::MeshState;
pub type KeepAwakeStateHandle = keepawake::KeepAwakeState;
pub type ResizeOwnerState = resize_owner::ResizeOwnerState;

pub use startup::{spawn_telegram_bridge, start_telegram_bridge};

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(any(test, feature = "test-support"))]
pub mod persistence_test_support;

pub use fdlimit::raise_fd_limit;
pub use instance_lifetime::acquire as acquire_instance_lifetime;
pub use instance_lifetime::{PathOwnership, ResolvedInstancePaths};
pub use liveness::start_api_liveness_watchdog;
pub use webview2_reap::reap_orphaned_webview2;

pub use webview2_reap::clear_journal_for_exit as clear_webview_journal_for_exit;

extern crate domios_account as codeterm_account; // rename-bridge
extern crate domios_agent_detect as codeterm_agent_detect; // rename-bridge
extern crate domios_agent_model as codeterm_agent_model; // rename-bridge
extern crate domios_analytics as codeterm_analytics; // rename-bridge
extern crate domios_billing as codeterm_billing; // rename-bridge
extern crate domios_blueprint_types as codeterm_blueprint_types; // rename-bridge
extern crate domios_bridge as codeterm_bridge; // rename-bridge
pub extern crate domios_browser as codeterm_browser; // rename-bridge
extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_config as codeterm_config; // rename-bridge
extern crate domios_crash as codeterm_crash; // rename-bridge
extern crate domios_cron as codeterm_cron; // rename-bridge
extern crate domios_decision as codeterm_decision; // rename-bridge
extern crate domios_events as codeterm_events; // rename-bridge
extern crate domios_frontend_bundle as codeterm_frontend_bundle; // rename-bridge
extern crate domios_frontend_shell as codeterm_frontend_shell; // rename-bridge
#[cfg(target_os = "macos")]
extern crate domios_input_macos as codeterm_input_macos; // rename-bridge
#[cfg(windows)]
extern crate domios_input_windows as codeterm_input_windows; // rename-bridge
extern crate domios_keepawake as codeterm_keepawake; // rename-bridge
extern crate domios_lanes as codeterm_lanes; // rename-bridge
extern crate domios_local_preview as codeterm_local_preview; // rename-bridge
extern crate domios_local_recording as codeterm_local_recording; // rename-bridge
extern crate domios_mem as codeterm_mem; // rename-bridge
extern crate domios_mem_store as codeterm_mem_store; // rename-bridge
extern crate domios_mesh as codeterm_mesh; // rename-bridge
extern crate domios_notification_core as codeterm_notification_core; // rename-bridge
extern crate domios_notifications as codeterm_notifications; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plan as codeterm_plan; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_plugins as codeterm_plugins; // rename-bridge
extern crate domios_prompts as codeterm_prompts; // rename-bridge
extern crate domios_provider_ops as codeterm_provider_ops; // rename-bridge
extern crate domios_provider_signals as codeterm_provider_signals; // rename-bridge
extern crate domios_pty as codeterm_pty; // rename-bridge
extern crate domios_recall as codeterm_recall; // rename-bridge
extern crate domios_repowatch as codeterm_repowatch; // rename-bridge
extern crate domios_secrets as codeterm_secrets; // rename-bridge
extern crate domios_sentinel as codeterm_sentinel; // rename-bridge
extern crate domios_shutdown as codeterm_shutdown; // rename-bridge
extern crate domios_storage as codeterm_storage; // rename-bridge
extern crate domios_supporter as codeterm_supporter; // rename-bridge
extern crate domios_surface as codeterm_surface; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_update as codeterm_update; // rename-bridge
extern crate domios_usage as codeterm_usage; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
extern crate domios_wire as codeterm_wire; // rename-bridge
extern crate domios_worker_services as codeterm_worker_services; // rename-bridge
