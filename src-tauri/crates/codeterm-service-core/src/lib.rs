pub mod agent;
pub mod agent_commands;
pub mod api_support;
pub mod app_runtime;
pub use codeterm_util::attachments;
mod autostart;
pub use autostart::{
    disable as disable_autostart, enable as enable_autostart, is_enabled as autostart_is_enabled,
};
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
#[cfg(windows)]
mod native_resize;
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
pub mod update_state;
pub mod usage;
pub mod user_last_activity;
pub mod view;
pub mod view_host;
mod webview2_reap;
mod webview_watchdog;

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
pub type UpdateStateHandle = Arc<Mutex<update_state::UpdateState>>;
pub type MeshStateHandle = mesh::MeshState;
pub type KeepAwakeStateHandle = keepawake::KeepAwakeState;
pub type ResizeOwnerState = resize_owner::ResizeOwnerState;

pub fn register_orchestration() {
    service::register_orchestration();
}

pub use startup::{spawn_telegram_bridge, start_telegram_bridge};

#[cfg(test)]
#[path = "../../../vendor/tauri-runtime-wry/src/owner_thread.rs"]
mod windows_rc_ownership_regression;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(any(test, feature = "test-support"))]
pub mod persistence_test_support;
