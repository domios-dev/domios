//! Application state initialization — builds [`AppStates`] from config.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::{self, ConfigState};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::notification::store::NotificationStore;
use crate::notification::NotifState;
use crate::pty::manager::{OutputBuffer, PtyManager};
use crate::pty::screen_state::ScreenState;
use crate::service::spawn::{should_default_agent_pane_to_chat, PaneVisibility};
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

    let pty_state: PtyState = Arc::new(Mutex::new(PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::new(500)));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState = Arc::new(Mutex::new(OutputBuffer::new(65536)));
    let screen_state: ScreenStateHandle = Arc::new(Mutex::new(ScreenState::new()));
    let agent_tracker: AgentTrackerState =
        Arc::new(Mutex::new(agent::status::PaneStatusTracker::new()));
    let report_store: ReportStoreState =
        Arc::new(Mutex::new(agent::reports::ReportStore::new(200)));
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
    if general_config.enabled {
        service::general_agent::ensure_directories();
        service::general_agent::generate_instructions(&general_config.agent_provider, None);
        let default_chat_view = default_chat_view_for_general_agent(
            config_state.lock().ui.default_new_agent_pane_chat,
            &general_config.agent_provider,
        );
        service::general_agent::ensure_general_workspace(&mut mux_state_inner, default_chat_view);
    }

    let mux_state: MuxState = Arc::new(Mutex::new(mux_state_inner));

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

fn default_chat_view_for_general_agent(default_enabled: bool, provider_id: &str) -> bool {
    should_default_agent_pane_to_chat(
        default_enabled,
        PaneVisibility::Visible,
        bundled_agent_provider_supports_chat_ui(provider_id),
    )
}

fn bundled_agent_provider_supports_chat_ui(provider_id: &str) -> bool {
    let registry = agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    registry.register_provider(Box::new(crate::plugin::agent_codex::CodexProvider::new()));
    registry.register_provider(Box::new(
        crate::plugin::agent_opencode::OpenCodeProvider::new(),
    ));
    registry
        .with_provider(provider_id, |provider| provider.supports_chat_ui())
        .unwrap_or(false)
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
}
