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

/// Every agent/worker pane the snapshot held — role tabs AND bare agent panes
/// (codex workers carry an `agent_type`, not always a role). This is the input
/// to the per-pane restore report, so it must be broader than role bindings.
fn snapshot_agent_panes(
    snapshot: &crate::session::store::SessionSnapshot,
) -> Vec<crate::crash::restore_report::RestorePane> {
    use crate::crash::restore_report::RestorePane;
    use crate::session::store::SplitNodeSnapshot;
    fn collect_node(node: &SplitNodeSnapshot, panes: &mut Vec<RestorePane>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                claude_session_id,
                role,
                agent_type,
                ..
            } if role.is_some() || agent_type.is_some() => panes.push(RestorePane {
                tab_id: tab_id.clone(),
                role: role.clone(),
                agent_type: agent_type.clone(),
                agent_session_id: claude_session_id.clone(),
            }),
            SplitNodeSnapshot::Leaf { .. } => {}
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect_node(child, panes);
                }
            }
        }
    }
    let mut panes = Vec::new();
    for tab in snapshot.workspaces.iter().flat_map(|ws| ws.tabs.iter()) {
        collect_node(&tab.split_tree, &mut panes);
    }
    panes
}

fn restored_agent_pane_ids(
    mux: &crate::multiplexer::state::MultiplexerState,
) -> std::collections::BTreeSet<String> {
    mux.snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role.is_some() || tab.agent_type.is_some())
        .map(|tab| tab.id.clone())
        .collect()
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

const INHERITED_IDENTITY_ENV_VARS: &[&str] = &[
    "CODETERM_ATTEMPT_ID",
    "CODETERM_LANE_BASE",
    "CODETERM_MACHINE_AGENT_IDENTITY",
    "CODETERM_PAD_DIR",
    "CODETERM_PANE_ID",
    "CODETERM_PARENT_TAB_ID",
    "CODETERM_PLAN_ID",
    "CODETERM_ROLE",
    "CODETERM_SESSION_BINDING_NONCE",
    "CODETERM_SESSION_ID",
    "CODETERM_SHELL_NONCE",
    "CODETERM_TAB_ID",
    "CODETERM_TAB_TITLE",
    "CODETERM_TASK_ID",
    "CODETERM_TEAM_ID",
    "CODETERM_WORKER_ID",
    "CODETERM_WORKSPACE_ID",
];

fn scrub_inherited_identity_env() {
    let mut scrubbed = Vec::new();
    for name in INHERITED_IDENTITY_ENV_VARS {
        if std::env::var_os(name).is_some() {
            std::env::remove_var(name);
            scrubbed.push(*name);
        }
    }
    if !scrubbed.is_empty() {
        log::info!(
            "[startup] scrubbed inherited CodeTerm identity env: {}",
            scrubbed.join(", ")
        );
    }
}

/// Scrub prod-targeting CodeTerm env inherited when dev starts inside prod.
/// PTYs inherit daemon env, so every child must see this instance's API target.
pub(crate) fn scrub_inherited_api_env(config_state: &ConfigState) {
    scrub_inherited_identity_env();
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
    let mut agent_panes_at_shutdown = Vec::new();
    let mux_state_inner = {
        let last = session_state.lock().load_last();
        match last {
            Ok(Some(snapshot)) if !snapshot.workspaces.is_empty() => {
                bound_at_shutdown = snapshot_role_bindings(&snapshot);
                agent_panes_at_shutdown = snapshot_agent_panes(&snapshot);
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
        Err(e) => log::error!(
            "[closed-history] open failed: {} — degrading to in-memory, reopen history will NOT survive restart",
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
    // Per-pane restore report: surface any agent/worker pane the snapshot held
    // that did not come back, instead of a silently-shorter workspace (#11). Only
    // when a session actually carried agent panes, so a fresh start does not
    // clobber the last report with an empty one.
    if !agent_panes_at_shutdown.is_empty() {
        let restored_pane_ids = restored_agent_pane_ids(&mux_state_inner);
        crate::crash::restore_report::record(&agent_panes_at_shutdown, &restored_pane_ids);
    }
    let mut duplicate_generals_closed = 0usize;
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
            duplicate_generals_closed =
                service::general_agent::ensure_general_workspace(&mut mux_state_inner, false);
        } else {
            service::general_agent::ensure_first_run_tab(&mut mux_state_inner);
        }
    }
    if ghost_count + duplicate_generals_closed > 0 {
        // Deliberate boot cleanups re-sync with intent; kept conditional so an
        // unexplained load-time loss still hits the non-reducing guard.
        let resync = session_state.lock().save_with_intent(
            &mux_state_inner.to_session_snapshot(),
            crate::session::store::SaveIntent::AuthorizedReduction,
        );
        if let Err(error) = resync {
            log::warn!("[restore] boot cleanup desk resync failed: {error}");
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

    fn isolated_startup_env() -> codeterm_util::test_env::EnvGuard {
        let mut env = codeterm_util::test_env::EnvGuard::lock();
        for name in INHERITED_IDENTITY_ENV_VARS.iter().copied().chain([
            "CODETERM_API_URL",
            "CODETERM_API_PORT",
            "CODETERM_API_TOKEN",
            "CODETERM_SOCKET_PATH",
            "CODETERM_INSTANCE",
            "CODETERM_DATA_DIR",
            "CODETERM_DEV",
        ]) {
            env.remove(name);
        }
        env
    }

    /// Dev launched inside prod must scrub inherited API env before PTYs spawn.
    #[test]
    fn scrub_overwrites_inherited_prod_api_env_with_instance_values() {
        let mut env = isolated_startup_env();
        for name in INHERITED_IDENTITY_ENV_VARS {
            env.set(name, format!("inherited-{name}"));
        }
        env.set("CODETERM_API_URL", "http://127.0.0.1:7685");
        env.set("CODETERM_API_PORT", "7685");
        env.set("CODETERM_API_TOKEN", "prod-token-inherited");
        env.set("CODETERM_SOCKET_PATH", "prod-inherited.sock");

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
        let expected_socket_path = crate::socket::server::socket_display_path();
        assert_eq!(
            std::env::var("CODETERM_SOCKET_PATH").ok().as_deref(),
            Some(expected_socket_path.as_str())
        );
        for name in INHERITED_IDENTITY_ENV_VARS {
            assert!(
                std::env::var(name).is_err(),
                "inherited identity variable {name} must be removed"
            );
        }
    }

    #[test]
    fn scrub_preserves_instance_env_and_later_daemon_identity() {
        let mut env = isolated_startup_env();
        env.set("CODETERM_TAB_ID", "inherited-tab");
        env.set("CODETERM_INSTANCE", "dev-test");
        env.set("CODETERM_DATA_DIR", "/tmp/codeterm-dev-test");
        env.set("CODETERM_DEV", "1");

        let mut cfg = crate::config::schema::AppConfig::default();
        cfg.api.port = 7686;
        let config_state: ConfigState = Arc::new(Mutex::new(cfg));

        scrub_inherited_api_env(&config_state);

        assert!(std::env::var("CODETERM_TAB_ID").is_err());
        assert_eq!(
            std::env::var("CODETERM_INSTANCE").ok().as_deref(),
            Some("dev-test")
        );
        assert_eq!(
            std::env::var("CODETERM_DATA_DIR").ok().as_deref(),
            Some("/tmp/codeterm-dev-test")
        );
        assert_eq!(std::env::var("CODETERM_DEV").ok().as_deref(), Some("1"));

        env.set("CODETERM_TAB_ID", "daemon-owned-tab");
        assert_eq!(
            std::env::var("CODETERM_TAB_ID").ok().as_deref(),
            Some("daemon-owned-tab"),
            "identity assigned after the startup scrub must remain intact"
        );
    }

    /// A tokenless instance must not leave a stale inherited prod token in env —
    /// otherwise a tab would present a foreign bearer that 401s everywhere.
    #[test]
    fn scrub_removes_inherited_token_when_instance_has_none() {
        let mut env = isolated_startup_env();
        env.set("CODETERM_API_TOKEN", "prod-token-inherited");

        let mut cfg = crate::config::schema::AppConfig::default();
        cfg.api.port = 7686;
        cfg.api.token = None;
        let config_state: ConfigState = Arc::new(Mutex::new(cfg));

        scrub_inherited_api_env(&config_state);

        assert!(
            std::env::var("CODETERM_API_TOKEN").is_err(),
            "tokenless instance must clear an inherited token, not keep prod's"
        );
    }
}
