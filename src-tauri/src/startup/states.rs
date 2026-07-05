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

    let mesh_config = config_state.lock().mesh.clone();
    let api_port = config_state.lock().api.port;
    let mesh_state: MeshStateHandle = Arc::new(Mutex::new(mesh::MeshManager::new(
        mesh_config.instance_name.clone(),
        api_port,
    )));

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
    let ghost_count = service::general_agent::purge_snapshot_ghosts(&mut mux_state_inner);
    if ghost_count > 0 {
        log::info!(
            "[restore] purged {} ephemeral ghost tab(s) from restored session snapshot",
            ghost_count
        );
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
            service::general_agent::ensure_general_workspace(&mut mux_state_inner, false);
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
