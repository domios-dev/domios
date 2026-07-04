//! Application state initialization — builds [`AppStates`] from config.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::{self, ConfigState};
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::notification::store::NotificationStore;
use crate::notification::NotifState;
use crate::pty::manager::{OutputBuffer, PtyManager};
use crate::pty::screen_state::ScreenState;
use crate::session::store::SessionStore;
use crate::session::SessionState;
use crate::{
    agent, api, keepawake, mesh, service, update, AgentTrackerState, KeepAwakeStateHandle,
    MeshStateHandle, OutputBufferState, PtyState, ReportStoreState, ResizeOwnerState,
    ScreenStateHandle, UpdateStateHandle,
};

/// All shared application states, grouped for easier passing between init functions.
pub(crate) struct AppStates {
    pub(crate) config: ConfigState,
    pub(crate) pty: PtyState,
    pub(crate) mux: MuxState,
    pub(crate) notif: NotifState,
    pub(crate) session: SessionState,
    pub(crate) output_buffer: OutputBufferState,
    pub(crate) screen_state: ScreenStateHandle,
    pub(crate) agent_tracker: AgentTrackerState,
    pub(crate) report_store: ReportStoreState,
    pub(crate) event_broadcaster: api::events::EventBroadcasterState,
    pub(crate) update: UpdateStateHandle,
    pub(crate) mesh: MeshStateHandle,
    pub(crate) keepawake: KeepAwakeStateHandle,
    pub(crate) resize_owner: ResizeOwnerState,
}

/// Create and initialize all application states.
pub(crate) fn init_states() -> AppStates {
    let app_config = config::load_config();
    log::info!("Config loaded: theme={}", app_config.theme.name);
    let config_state: ConfigState = Arc::new(Mutex::new(app_config));

    let pty_state: PtyState = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::new(500)));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65536)));
    let screen_state: ScreenStateHandle =
        Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let agent_tracker: AgentTrackerState = Arc::new(TimedMutex::new(
        "agent_tracker",
        agent::status::PaneStatusTracker::new(),
    ));
    // Durable orchestration ledger backs the report store so worker state
    // survives daemon restart. If the db can't be opened, fall back to an
    let report_store: ReportStoreState = {
        let store = match agent::ledger::OrchestrationLedger::open(&crate::paths::data_dir()) {
            Ok(ledger) => agent::reports::ReportStore::with_ledger(200, Arc::new(ledger)),
            Err(e) => {
                log::warn!("[ledger] open failed: {} — reports will not persist", e);
                agent::reports::ReportStore::new(200)
            }
        };
        Arc::new(Mutex::new(store))
    };
    let event_broadcaster: api::events::EventBroadcasterState =
        Arc::new(api::events::EventBroadcaster::new());
    let resize_owner: ResizeOwnerState = Arc::new(Mutex::new(
        crate::resize_owner::ResizeOwnerRegistry::default(),
    ));

    // Initialize mesh state
    let mesh_config = config_state.lock().mesh.clone();
    let api_port = config_state.lock().api.port;
    let mesh_state: MeshStateHandle = Arc::new(Mutex::new(mesh::MeshManager::new(
        mesh_config.instance_name.clone(),
        api_port,
    )));

    // Detect install method and init update state
    let install_method = update::detect_install_method();
    log::info!("Install method detected: {}", install_method);
    let channel = config_state.lock().updater.channel;
    let update_state: UpdateStateHandle = Arc::new(Mutex::new(update::UpdateState {
        install_method,
        last_check: None,
        last_check_time: None,
        channel,
        phase: update::UpdatePhase::Idle,
        last_error: None,
        phase_changed_at: None,
        downloaded_bytes: 0,
        download_total_bytes: None,
    }));

    // Try to restore last session, fallback to default workspace
    let ga_enabled = config_state.lock().general_agent.enabled;
    let mux_state_inner = {
        let last = session_state.lock().load_last();
        match last {
            Ok(Some(snapshot)) if !snapshot.workspaces.is_empty() => {
                log::info!(
                    "Restoring session with {} workspaces, {} tabs total",
                    snapshot.workspaces.len(),
                    snapshot
                        .workspaces
                        .iter()
                        .map(|w| w.tabs.len())
                        .sum::<usize>()
                );
                for ws in &snapshot.workspaces {
                    log::info!(
                        "  Workspace '{}': {} tabs, active_tab={:?}",
                        ws.name,
                        ws.tabs.len(),
                        ws.active_tab_id
                    );
                }
                MultiplexerState::from_session_snapshot(&snapshot)
            }
            _ => {
                if ga_enabled {
                    // GA will create its own workspace — start empty
                    log::info!(
                        "No saved session, GA enabled — starting empty (General will be created)"
                    );
                    MultiplexerState::empty()
                } else {
                    log::info!("No saved session, creating default workspace");
                    let (state, _) = MultiplexerState::new();
                    state
                }
            }
        }
    };
    // Ensure General Agent workspace exists (before any threading/locking — no deadlock)
    let general_config = config_state.lock().general_agent.clone();
    #[allow(unused_mut)]
    let mut mux_state_inner = mux_state_inner;

    // Attach the durable closed-tab history store so reopen survives restarts.
    // On failure keep the in-memory default (history just won't persist).
    match crate::multiplexer::state::ClosedPanesStore::open(&crate::paths::data_dir()) {
        Ok(store) => mux_state_inner.attach_closed_store(store),
        Err(e) => log::warn!(
            "[closed-history] open failed: {} — history will not persist",
            e
        ),
    }
    if general_config.enabled {
        service::general_agent::ensure_directories();
        let mut general_config = general_config;
        if !general_config.first_run_complete
            && service::general_agent::snapshot_has_protected_main(&mux_state_inner)
        {
            general_config.first_run_complete = true;
            let mut cfg = config_state.lock();
            cfg.general_agent.first_run_complete = true;
            let snapshot = cfg.clone();
            drop(cfg);
            if let Err(e) = crate::config::save_config_pub(&snapshot) {
                log::warn!("[general-agent] first_run migration save failed: {}", e);
            }
        }
        if general_config.first_run_complete {
            service::general_agent::generate_instructions(&general_config.agent_provider, None);
            let default_chat_view = service::general_agent::default_chat_view_for_provider(
                &config_state.lock(),
                &general_config.agent_provider,
            );
            service::general_agent::ensure_general_workspace(
                &mut mux_state_inner,
                default_chat_view,
            );
        } else {
            service::general_agent::ensure_first_run_tab(&mut mux_state_inner);
        }
    }

    let mux_state: MuxState = Arc::new(TimedRwLock::new("mux", mux_state_inner));

    AppStates {
        config: config_state,
        pty: pty_state,
        mux: mux_state,
        notif: notif_state,
        session: session_state,
        output_buffer,
        screen_state,
        agent_tracker,
        report_store,
        event_broadcaster,
        update: update_state,
        mesh: mesh_state,
        keepawake: keepawake::new_state(),
        resize_owner,
    }
}

#[cfg(test)]
fn default_chat_view_for_general_agent(default_enabled: bool, provider_id: &str) -> bool {
    crate::service::spawn::should_default_agent_pane_to_chat(
        default_enabled,
        crate::service::spawn::PaneVisibility::Visible,
        bundled_agent_provider_supports_chat_ui(provider_id),
    )
}

/// Startup bootstrap allowlist of chat-UI-capable bundled providers.
///
/// This is documented debt, not a leak we can close yet. The honest capability
/// (`AgentProvider::supports_chat_ui`) is derived at RUNTIME from a loaded
/// plugin VM (`has_js("sessionJsonlPath") || uses_structured_chat() || …`), so
/// it cannot be computed statically. And `init_states` runs (lib.rs) *before*
/// the agent registry is created and the embedded plugins are bootstrapped to
/// disk and loaded — so there is no provider to query here. Until chat-view
/// defaulting is deferred to a post-registry startup hook (see the loop notes),
/// this single list is the one source of truth, cross-checked against the
/// embedded plugin set by the test below so it can't name a provider we no
/// longer ship.
#[cfg(test)]
const BUNDLED_CHAT_UI_PROVIDERS: &[&str] = &["claude", "codex", "opencode", "cursor"];

#[cfg(test)]
fn bundled_agent_provider_supports_chat_ui(provider_id: &str) -> bool {
    BUNDLED_CHAT_UI_PROVIDERS.contains(&provider_id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn general_agent_chat_default_on_capable_provider_sets_chat_view() {
        assert!(super::default_chat_view_for_general_agent(true, "claude"));
    }

    #[test]
    fn general_agent_chat_default_off_capable_provider_stays_terminal() {
        assert!(!super::default_chat_view_for_general_agent(false, "claude"));
    }

    #[test]
    fn general_agent_chat_default_on_unknown_provider_stays_terminal() {
        assert!(!super::default_chat_view_for_general_agent(true, "unknown"));
    }

    /// Drift guard: every provider in the startup chat-UI bootstrap allowlist
    /// must actually ship as an embedded plugin (`provider-<id>`). This uses
    /// the one capability source available before the registry loads — the
    /// statically-embedded plugin set — so the allowlist can't silently
    /// reference a provider that was removed from the build (which would
    /// default panes to a chat view no loaded provider serves).
    #[test]
    fn chat_ui_allowlist_entries_are_all_embedded_plugins() {
        let embedded = crate::plugin::embedded::ids();
        for id in super::BUNDLED_CHAT_UI_PROVIDERS {
            assert!(
                embedded.contains(id),
                "chat-UI allowlist names '{id}' but it is not an embedded plugin (embedded: {embedded:?})"
            );
        }
    }
}
