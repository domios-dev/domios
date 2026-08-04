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
    pub(crate) rest_api: super::RestApiControlState,
}

/// Reduce the snapshot to the authority the lifecycle journal proves. A tab
/// whose claim has no exact committed checkpoint comes back as a plain shell;
/// the journal and the snapshot file both keep the forensic record.
fn quarantine_unproven_authority(snapshot: &mut crate::session::store::SessionSnapshot) {
    use crate::session::lifecycle::{
        gate_session_snapshot, RecoveryVerdict, TabLifecycleJournal,
    };

    let load = TabLifecycleJournal::shared().load();
    if let crate::session::lifecycle::TailStatus::Refused { line, reason } = &load.tail {
        log::error!(
            "[tab-lifecycle] journal unreadable from line {line} ({reason}); every authority claim fails closed"
        );
    }
    for outcome in gate_session_snapshot(snapshot, &load) {
        match outcome.verdict {
            RecoveryVerdict::Stripped(refusal) => log::warn!(
                "[tab-lifecycle] tab {} restored without authority: {}",
                outcome.tab_id,
                refusal.reason()
            ),
            RecoveryVerdict::Restored(authority) => log::info!(
                "[tab-lifecycle] tab {} restored from committed checkpoint (provider={}, session={})",
                outcome.tab_id,
                authority.provider,
                authority.session_id
            ),
            RecoveryVerdict::NoAuthorityClaimed => {}
        }
    }
}

/// Orchestration bindings the dead epoch persisted, versus what came back.
/// Roleless tabs are ordinary terminals, not in-flight work.
fn snapshot_role_bindings(
    snapshot: &crate::session::store::SessionSnapshot,
) -> Vec<crate::crash::lifecycle::BindingIdentity> {
    use crate::session::store::SplitNodeSnapshot;
    fn collect_node(
        node: &SplitNodeSnapshot,
        bindings: &mut Vec<crate::crash::lifecycle::BindingIdentity>,
    ) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                claude_session_id,
                role,
                ..
            } if role.is_some() => bindings.push(crate::crash::lifecycle::BindingIdentity {
                tab_id: tab_id.clone(),
                agent_session_id: claude_session_id.clone(),
            }),
            SplitNodeSnapshot::Leaf { .. } => {}
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect_node(child, bindings);
                }
            }
        }
    }
    let mut bindings = Vec::new();
    for tab in snapshot.workspaces.iter().flat_map(|ws| ws.tabs.iter()) {
        collect_node(&tab.split_tree, &mut bindings);
    }
    bindings
}

fn restored_role_bindings(
    mux: &crate::multiplexer::state::MultiplexerState,
) -> Vec<crate::crash::lifecycle::BindingIdentity> {
    mux.snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role.is_some())
        .map(|tab| crate::crash::lifecycle::BindingIdentity {
            tab_id: tab.id.clone(),
            agent_session_id: tab.agent_session_id.clone(),
        })
        .collect()
}

/// Scrub prod-targeting CodeTerm env inherited when dev starts inside prod.
/// PTYs inherit daemon env, so every child must see this instance's API target.
pub(crate) fn scrub_inherited_api_env(config_state: &ConfigState) {
    let api = config_state.lock().api.clone();
    let (url, port) = config::api_callback_url(&api);
    std::env::set_var("CODETERM_API_URL", &url);
    std::env::set_var("CODETERM_API_PORT", &port);
    std::env::set_var(
        "CODETERM_SOCKET_PATH",
        crate::socket::server::socket_display_path(),
    );
    match api.token {
        Some(ref tok) if !tok.is_empty() => std::env::set_var("CODETERM_API_TOKEN", tok),
        _ => std::env::remove_var("CODETERM_API_TOKEN"),
    }
    log::info!("[startup] scrubbed inherited CODETERM_API_* env -> {}", url);
}

/// Create and initialize all application states.
pub(crate) fn init_states() -> AppStates {
    let app_config = config::load_config();
    log::info!("Config loaded: theme={}", app_config.theme.name);
    let config_state: ConfigState = Arc::new(Mutex::new(app_config));
    scrub_inherited_api_env(&config_state);

    let pty_state: PtyState = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::new(500)));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65536)));
    let screen_state: ScreenStateHandle =
        Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let agent_tracker: AgentTrackerState = Arc::new(TimedMutex::new(
        "agent_tracker",
        agent::status::TabStatusTracker::new(),
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
    let rest_api = Arc::new(super::RestApiControl::new());

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
    let mut bound_at_shutdown = Vec::new();
    let mux_state_inner = {
        let last = session_state.lock().load_last();
        match last {
            Ok(Some(mut snapshot)) if !snapshot.workspaces.is_empty() => {
                quarantine_unproven_authority(&mut snapshot);
                bound_at_shutdown = snapshot_role_bindings(&snapshot);
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
    match crate::multiplexer::state::ClosedTabsStore::open(&crate::paths::data_dir()) {
        Ok(store) => mux_state_inner.attach_closed_store(store),
        Err(e) => log::warn!(
            "[closed-history] open failed: {} — history will not persist",
            e
        ),
    }
    let ghost_count = service::general_agent::purge_snapshot_ghosts(&mut mux_state_inner);
    let restored_agent_tabs = mux_state_inner
        .snapshot()
        .tabs
        .into_values()
        .filter(|tab| tab.agent_type.is_some() || tab.role.is_some() || tab.is_chat_backend())
        .map(|tab| {
            let role = service::agent_pad::pad_role_for_tab(&tab);
            (tab.id, role)
        })
        .collect::<Vec<_>>();
    for (tab_id, role) in &restored_agent_tabs {
        if let Err(error) = service::agent_pad::restore(&crate::paths::data_dir(), tab_id, *role) {
            log::warn!("[agent-pad] restore failed for Tab {tab_id}: {error}");
        }
    }
    let live_agent_tabs = restored_agent_tabs
        .into_iter()
        .map(|(tab_id, _)| tab_id)
        .collect();
    if let Err(error) = service::agent_pad::reconcile(&crate::paths::data_dir(), &live_agent_tabs) {
        log::warn!("[agent-pad] startup reconciliation failed: {error}");
    }
    if ghost_count > 0 {
        log::info!(
            "[restore] purged {} ephemeral ghost tab(s) from restored session snapshot",
            ghost_count
        );
    }
    let restored_bindings = restored_role_bindings(&mux_state_inner);
    crate::crash::lifecycle::note_restored_bindings(crate::crash::lifecycle::BindingEvidence {
        bound_at_shutdown: bound_at_shutdown.len() as u32,
        restored: restored_bindings.len() as u32,
        bound_identities: bound_at_shutdown,
        restored_identities: restored_bindings,
    });
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
        rest_api,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serialize process-env mutation across tests in this binary.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    /// Dev launched inside prod must scrub inherited API env before PTYs spawn.
    #[test]
    fn scrub_overwrites_inherited_prod_api_env_with_instance_values() {
        let _guard = ENV_GUARD.lock();
        std::env::set_var("CODETERM_API_URL", "http://127.0.0.1:7685");
        std::env::set_var("CODETERM_API_PORT", "7685");
        std::env::set_var("CODETERM_API_TOKEN", "prod-token-inherited");

        let mut cfg = crate::config::schema::AppConfig::default();
        cfg.api.host = "0.0.0.0".to_string();
        cfg.api.port = 7686;
        cfg.api.token = Some("dev-token".to_string());
        let config_state: ConfigState = Arc::new(Mutex::new(cfg));

        scrub_inherited_api_env(&config_state);

        assert_eq!(
            std::env::var("CODETERM_API_URL").ok().as_deref(),
            Some("http://127.0.0.1:7686"),
            "inherited prod URL must be replaced by the running instance URL"
        );
        assert_eq!(
            std::env::var("CODETERM_API_PORT").ok().as_deref(),
            Some("7686")
        );
        assert_eq!(
            std::env::var("CODETERM_API_TOKEN").ok().as_deref(),
            Some("dev-token"),
            "inherited prod token must be replaced by the running instance token"
        );

        std::env::remove_var("CODETERM_API_URL");
        std::env::remove_var("CODETERM_API_PORT");
        std::env::remove_var("CODETERM_API_TOKEN");
    }

    /// A tokenless instance must not leave a stale inherited prod token in env —
    /// otherwise a tab would present a foreign bearer that 401s everywhere.
    #[test]
    fn scrub_removes_inherited_token_when_instance_has_none() {
        let _guard = ENV_GUARD.lock();
        std::env::set_var("CODETERM_API_TOKEN", "prod-token-inherited");

        let mut cfg = crate::config::schema::AppConfig::default();
        cfg.api.port = 7686;
        cfg.api.token = None;
        let config_state: ConfigState = Arc::new(Mutex::new(cfg));

        scrub_inherited_api_env(&config_state);

        assert!(
            std::env::var("CODETERM_API_TOKEN").is_err(),
            "tokenless instance must clear an inherited token, not keep prod's"
        );
        std::env::remove_var("CODETERM_API_URL");
        std::env::remove_var("CODETERM_API_PORT");
        std::env::remove_var("CODETERM_SOCKET_PATH");
    }
}
