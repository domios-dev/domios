//! Application state initialization — builds [`AppStates`] from config.

use std::sync::Arc;

use parking_lot::Mutex;

use codeterm_service_core::config::{self, ConfigState};
use codeterm_service_core::lock_hold::{TimedMutex, TimedRwLock};
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::notification::store::NotificationStore;
use codeterm_service_core::notification::NotifState;
use codeterm_service_core::pty::manager::{OutputBuffer, PtyManager};
use codeterm_service_core::pty::screen_state::ScreenState;
use codeterm_service_core::session::store::SessionStore;
use codeterm_service_core::session::SessionState;
use codeterm_service_core::startup::StorageAuthority;
use codeterm_service_core::{
    agent, keepawake, mesh, service, AgentTrackerState, MeshStateHandle, OutputBufferState,
    PtyState, ReportStoreState, ResizeOwnerState, ScreenStateHandle,
};
use codeterm_update::UpdateStateHandle;

use codeterm_service_core::startup::AppStates;

/// Orchestration bindings the dead epoch persisted, versus what came back.
/// Roleless tabs are ordinary terminals, not in-flight work.
fn snapshot_role_bindings(
    snapshot: &codeterm_service_core::session::store::SessionSnapshot,
) -> Vec<codeterm_service_core::crash::lifecycle::BindingIdentity> {
    use codeterm_service_core::session::store::SplitNodeSnapshot;
    fn collect_node(
        node: &SplitNodeSnapshot,
        bindings: &mut Vec<codeterm_service_core::crash::lifecycle::BindingIdentity>,
    ) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_session_id,
                role,
                ..
            } if role.is_some() => {
                bindings.push(codeterm_service_core::crash::lifecycle::BindingIdentity {
                    tab_id: tab_id.clone(),
                    agent_session_id: agent_session_id.clone(),
                })
            }
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
/// (some providers carry an `agent_type`, not always a role). This is the input
/// to the per-pane restore report, so it must be broader than role bindings.
fn snapshot_agent_panes(
    snapshot: &codeterm_service_core::session::store::SessionSnapshot,
) -> Vec<codeterm_service_core::crash::restore_report::RestorePane> {
    use codeterm_service_core::crash::restore_report::RestorePane;
    use codeterm_service_core::session::store::SplitNodeSnapshot;
    fn collect_node(node: &SplitNodeSnapshot, panes: &mut Vec<RestorePane>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_session_id,
                role,
                agent_type,
                ..
            } if role.is_some() || agent_type.is_some() => panes.push(RestorePane {
                tab_id: tab_id.clone(),
                role: role.clone(),
                agent_type: agent_type.clone(),
                agent_session_id: agent_session_id.clone(),
                exited_session_id: None,
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

/// Which agent/worker tab RECORDS came back. Structural only: this answers
/// "did the record survive restore", never "is an agent running behind it".
/// The liveness verdict is observed separately and settles later.
fn restored_agent_pane_ids(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
) -> std::collections::BTreeSet<String> {
    mux.snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role.is_some() || tab.agent_type.is_some())
        .map(|tab| tab.id.clone())
        .collect()
}

fn restored_role_bindings(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
) -> Vec<codeterm_service_core::crash::lifecycle::BindingIdentity> {
    mux.snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role.is_some())
        .map(
            |tab| codeterm_service_core::crash::lifecycle::BindingIdentity {
                tab_id: tab.id.clone(),
                agent_session_id: tab.agent_session_id.clone(),
            },
        )
        .collect()
}

fn reconcile_startup_teams(
    snapshot: &mut codeterm_service_core::session::store::SessionSnapshot,
    boot_resync: &mut bool,
) {
    let orphans = codeterm_service_core::session::store::prune_orphan_teams(snapshot);
    for team in &orphans {
        log::warn!(
            "[restore] dropped orphan Team {}: workspace {} is missing",
            team.id,
            team.workspace_id
        );
    }
    if !orphans.is_empty() {
        *boot_resync = true;
    }
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
            "[startup] scrubbed inherited Domios identity env: {}",
            scrubbed.join(", ")
        );
    }
}

/// Scrub prod-targeting Domios env inherited when dev starts inside prod.
/// PTYs inherit daemon env, so every child must see this instance's API target.
pub fn scrub_inherited_api_env(config_state: &ConfigState) {
    scrub_inherited_identity_env();
    let api = config_state.lock().api.clone();
    let (url, port) = config::api_callback_url(&api);
    std::env::set_var("CODETERM_API_URL", &url);
    std::env::set_var("CODETERM_API_PORT", &port);
    std::env::set_var(
        "CODETERM_SOCKET_PATH",
        codeterm_service_core::paths::socket_display_path(),
    );
    match api.token {
        Some(ref tok) if !tok.is_empty() => std::env::set_var("CODETERM_API_TOKEN", tok),
        _ => std::env::remove_var("CODETERM_API_TOKEN"),
    }
    log::info!("[startup] scrubbed inherited CODETERM_API_* env -> {}", url);
}

pub(crate) fn init_config() -> ConfigState {
    let mut app_config = config::load_config();
    if std::env::var_os("CODETERM_VERIFY_ISOLATED").as_deref() == Some(std::ffi::OsStr::new("1")) {
        app_config.api.port = std::env::var("CODETERM_API_PORT")
            .ok()
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| !matches!(port, 0 | 7685 | 7686))
            .unwrap_or_else(|| {
                log::error!("[startup] isolated verification port is invalid");
                eprintln!("Domios startup refused: isolated verification port is invalid");
                std::process::exit(2);
            });
        if app_config
            .api
            .token
            .as_deref()
            .is_none_or(|token| token.trim().is_empty())
        {
            log::error!("[startup] isolated verification requires an API token");
            eprintln!("Domios startup refused: isolated verification requires an API token");
            std::process::exit(2);
        }
        super::listener_policy::apply_isolated_listener_policy(&mut app_config);
    } else {
        app_config.api.port = codeterm_service_core::paths::runtime_api_port(
            app_config.api.port,
            std::env::var("CODETERM_API_PORT").ok().as_deref(),
            cfg!(debug_assertions),
        )
        .unwrap_or_else(|error| {
            log::error!("[startup] refusing unsafe instance port: {error}");
            eprintln!("Domios startup refused: {error}");
            std::process::exit(2);
        });
    }
    log::info!("Config loaded: theme={}", app_config.theme.name);
    let config_state: ConfigState = Arc::new(Mutex::new(app_config));
    scrub_inherited_api_env(&config_state);
    config_state
}

pub(crate) fn init_states(
    config_state: ConfigState,
    rest_api: codeterm_service_core::startup::RestApiControlState,
) -> AppStates {
    let mut storage = codeterm_service_core::startup::open_instance_storage();

    let pty_state: PtyState = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::open_persisted(
        codeterm_service_core::paths::data_dir()
            .join(codeterm_service_core::notification::persist::FILE_NAME),
        500,
    )));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65536)));
    let screen_state: ScreenStateHandle = Arc::new(ScreenState::new());
    let agent_tracker: AgentTrackerState = Arc::new(TimedMutex::new(
        "agent_tracker",
        agent::status::TabStatusTracker::new(),
    ));
    let report_store: ReportStoreState = {
        let store = match agent::ledger::OrchestrationLedger::open(
            &codeterm_service_core::paths::data_dir(),
        ) {
            Ok(ledger) => agent::reports::ReportStore::with_ledger(200, Arc::new(ledger)),
            Err(e) => {
                log::warn!("[ledger] open failed: {} — reports will not persist", e);
                agent::reports::ReportStore::new(200)
            }
        };
        Arc::new(Mutex::new(store))
    };
    let event_broadcaster: codeterm_events::EventBroadcasterState =
        Arc::new(codeterm_events::EventBroadcaster::new());
    let resize_owner: ResizeOwnerState = Arc::new(Mutex::new(
        codeterm_service_core::resize_owner::ResizeOwnerRegistry::default(),
    ));

    let mesh_config = config_state.lock().mesh.clone();
    let api_port = config_state.lock().api.port;
    let mut mesh_manager = mesh::MeshManager::new(mesh_config.instance_name.clone(), api_port);
    mesh_manager.await_account_state();
    let mesh_state: MeshStateHandle = Arc::new(Mutex::new(mesh_manager));

    let install_method = codeterm_update::update_state::detect_install_method();
    log::info!("Install method detected: {}", install_method);
    let channel = config_state.lock().updater.channel;
    let update_state: UpdateStateHandle =
        Arc::new(Mutex::new(codeterm_update::update_state::UpdateState {
            install_method,
            last_check: None,
            last_check_time: None,
            channel,
            phase: codeterm_update::update_state::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        }));

    let ga_enabled = storage.storage().is_some() && config_state.lock().general_agent.enabled;
    let mut bound_at_shutdown = Vec::new();
    let mut agent_panes_at_shutdown = Vec::new();
    let mut desk_snapshot = match storage.storage() {
        Some(storage_owner) => {
            let adapter = codeterm_service_core::session::store::StorageDeskAdapter::from_storage(
                storage_owner.clone(),
            );
            match adapter.snapshot() {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    storage.mark_unavailable(format!("desk restore failed: {error}"));
                    None
                }
            }
        }
        None => None,
    };
    let mut boot_resync = false;
    if let Some(snapshot) = desk_snapshot.as_mut() {
        reconcile_startup_teams(snapshot, &mut boot_resync);
    }
    let mux_state_inner = match (storage.storage().is_some(), desk_snapshot.as_ref()) {
        (true, Some(snapshot)) if !snapshot.workspaces.is_empty() => {
            bound_at_shutdown = snapshot_role_bindings(snapshot);
            agent_panes_at_shutdown = snapshot_agent_panes(snapshot);
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
            MultiplexerState::from_session_snapshot(snapshot)
        }
        _ => initial_empty_desk_state(&storage, ga_enabled, &mut boot_resync),
    };
    // Ensure General Agent workspace exists (before any threading/locking — no deadlock)
    let general_config = config_state.lock().general_agent.clone();
    #[allow(unused_mut)]
    let mut mux_state_inner = mux_state_inner;
    if storage.storage().is_some() {
        for (tab_id, reason) in service::mark_legacy_role_shell_failures(&mut mux_state_inner) {
            log::warn!("[restore] Tab {tab_id} remains a role shell: {reason}");
        }
    }

    // Attach the durable closed-tab history store so reopen survives restarts.
    // On failure keep the in-memory default (history just won't persist).
    if storage.storage().is_some() {
        match codeterm_service_core::multiplexer::state::ClosedTabsStore::open(&codeterm_service_core::paths::data_dir()) {
            Ok(store) => mux_state_inner.attach_closed_store(store),
            Err(e) => log::error!(
                "[closed-history] open failed: {} — degrading to in-memory, reopen history will NOT survive restart",
                e
            ),
        }
    }
    let (ghost_count, ghost_evicted) = if storage.storage().is_some() {
        service::general_agent::purge_snapshot_ghosts_with_evictions(&mut mux_state_inner)
    } else {
        (0, Vec::new())
    };
    if let Some(storage) = storage.storage().cloned() {
        service::tab_todos::forget_permanently_closed_todos(storage, ghost_evicted);
    }
    let retired = if storage.storage().is_some() {
        notif_state
            .lock()
            .retire_tabless_conditions(|tab_id| mux_state_inner.get_tab(tab_id).is_some())
    } else {
        Vec::new()
    };
    if !retired.is_empty() {
        log::info!(
            "[notify-persist] resolved {} restored notice(s) whose Tab did not come back",
            retired.len()
        );
    }
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
    if storage.storage().is_some() {
        for (tab_id, role) in &restored_agent_tabs {
            if let Err(error) = service::agent_pad::restore(
                &codeterm_service_core::paths::data_dir(),
                tab_id,
                *role,
            ) {
                log::warn!("[agent-pad] restore failed for Tab {tab_id}: {error}");
            }
        }
    }
    let live_agent_tabs = restored_agent_tabs
        .into_iter()
        .map(|(tab_id, _)| tab_id)
        .collect();
    if storage.storage().is_some() {
        if let Err(error) = service::agent_pad::reconcile(
            &codeterm_service_core::paths::data_dir(),
            &live_agent_tabs,
        ) {
            log::warn!("[agent-pad] startup reconciliation failed: {error}");
        }
    }
    if ghost_count > 0 {
        log::info!(
            "[restore] purged {} ephemeral ghost tab(s) from restored session snapshot",
            ghost_count
        );
    }
    if storage.storage().is_some() {
        for tab in mux_state_inner
            .all_tabs()
            .filter(|tab| tab.restored_from_snapshot)
        {
            codeterm_service_core::service::supporter_record_launch(&tab.id, true);
        }
    }
    let restored_bindings = restored_role_bindings(&mux_state_inner);
    if storage.storage().is_some() {
        codeterm_service_core::crash::lifecycle::note_restored_bindings(
            codeterm_service_core::crash::lifecycle::BindingEvidence {
                bound_at_shutdown: bound_at_shutdown.len() as u32,
                restored: restored_bindings.len() as u32,
                bound_identities: bound_at_shutdown,
                restored_identities: restored_bindings,
            },
        );
    }
    // Nothing is observable yet — no PTY has materialized — so the wave is
    // armed here and settles on real launch transitions.
    if storage.storage().is_some() {
        service::apply_restore_decisions(
            codeterm_service_core::session::lifecycle::TabLifecycleJournal::shared(),
            &mux_state_inner,
            &mut agent_panes_at_shutdown,
        );
    }
    if storage.storage().is_some() && !agent_panes_at_shutdown.is_empty() {
        let restored_pane_ids = restored_agent_pane_ids(&mux_state_inner);
        codeterm_service_core::crash::restore_report::arm(
            codeterm_service_core::paths::data_dir(),
            agent_panes_at_shutdown,
            &restored_pane_ids,
        );
    }
    let mut duplicate_generals_closed = 0usize;
    if storage.storage().is_some() && general_config.enabled {
        service::general_agent::ensure_directories();
        if general_config.first_run_complete {
            service::general_agent::generate_instructions(
                general_config.capability_hints,
                &general_config.agent_provider,
                None,
            );
            let (closed, evicted) = service::general_agent::ensure_general_workspace_with_evictions(
                &mut mux_state_inner,
                false,
            );
            duplicate_generals_closed = closed;
            if let Some(storage) = storage.storage().cloned() {
                service::tab_todos::forget_permanently_closed_todos(storage, evicted);
            }
        } else {
            service::general_agent::ensure_first_run_tab(&mut mux_state_inner);
        }
    }
    if ghost_count + duplicate_generals_closed > 0 {
        // Deliberate boot cleanups re-sync with intent; kept conditional so an
        // unexplained load-time loss still hits the non-reducing guard.
        boot_resync = true;
    }

    if storage.storage().is_some() {
        for (tab_id, reason) in codeterm_service_core::session::load_restored_scrollback_tails(
            &mut mux_state_inner,
            &session_state,
        ) {
            log::debug!("[restore-scrollback] tab={tab_id} skipped reason={reason}");
        }
    }

    for workspace in mux_state_inner.list_workspaces() {
        let missing =
            codeterm_service_core::service::spawn_cwd::validate_spawn_cwd(workspace.cwd.as_deref())
                .is_err();
        if let Some(ws) = mux_state_inner.get_workspace_mut(&workspace.id) {
            ws.cwd_missing = missing;
        }
    }
    mux_state_inner.set_limit_gate(codeterm_service_core::service::billing::pending_limit_gate());
    let mux_state: MuxState = Arc::new(TimedRwLock::new("mux", mux_state_inner));

    AppStates {
        startup_desk: codeterm_service_core::startup::desk::StartupDesk::new(
            mux_state.clone(),
            event_broadcaster.clone(),
        ),
        storage,
        boot_resync,
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

fn initial_empty_desk_state(
    storage: &StorageAuthority,
    ga_enabled: bool,
    boot_resync: &mut bool,
) -> MultiplexerState {
    if storage.storage().is_none() {
        log::error!("Storage unavailable — desk restore and default workspace creation skipped");
        return MultiplexerState::empty();
    }

    *boot_resync = true;
    if ga_enabled {
        log::info!("Storage desk is empty, GA enabled — General will be created");
        MultiplexerState::empty()
    } else {
        log::info!("Storage desk is empty, creating default workspace");
        MultiplexerState::new().0
    }
}

#[cfg(test)]
mod tests;
