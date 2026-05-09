mod agent;
mod api;
pub mod attachments;
mod autostart;
pub mod bridge;
mod clipboard_files;
mod commands;
pub mod dispatch;
mod commands_mesh;
mod keepawake;
mod config;
pub mod cron;
pub mod mem;
pub mod mesh;
pub mod multiplexer;
mod notification;
pub mod paths;
pub mod plugin;
pub mod prompts;
pub mod process_util;
pub mod pty;
pub mod secrets;
pub mod service;
pub mod services;
pub mod shell;
pub mod session;
mod socket;
mod startup;
pub mod update;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use multiplexer::commands::MuxState;
use notification::NotifState;
use pty::manager::{OutputBuffer, PtyManager};
use pty::screen_state::ScreenState;
use service::{ServiceContext, ServiceState};
use session::SessionState;

pub type PtyState = Arc<Mutex<PtyManager>>;
pub type OutputBufferState = Arc<Mutex<OutputBuffer>>;
pub type ScreenStateHandle = Arc<Mutex<ScreenState>>;
pub type AgentTrackerState = Arc<Mutex<agent::status::PaneStatusTracker>>;
pub type ReportStoreState = Arc<Mutex<agent::reports::ReportStore>>;
pub type PromptDetectorState = Arc<Mutex<agent::prompt::PromptDetector>>;
pub use cron::scheduler::{CronState, CronWake};
pub type UpdateStateHandle = Arc<Mutex<update::UpdateState>>;
pub type MeshStateHandle = mesh::MeshState;
pub type KeepAwakeStateHandle = keepawake::KeepAwakeState;

pub use startup::{spawn_telegram_bridge, start_telegram_bridge};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init,
    // otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    startup::setup_logging();

    log::info!("CodeTerm starting...");

    // Write shell integration scripts (OSC 7 CWD tracking)
    pty::shell_integration::ensure_scripts();

    // Initialize all shared states
    let states = startup::init_states();

    // Seed bundled skills to ~/.codeterm/skills/ (sync watcher mirrors to providers)
    commands::seed_bundled_skills();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin({
            let mut updater = tauri_plugin_updater::Builder::new();
            if let Some(token) = option_env!("UPDATER_TOKEN") {
                if !token.is_empty() {
                    updater = updater
                        .header("Authorization", format!("token {}", token))
                        .unwrap()
                        .header("Accept", "application/octet-stream")
                        .unwrap();
                }
            }
            updater.build()
        });

    // WebDriver plugin for E2E UI testing (cargo build --features webdriver)
    #[cfg(feature = "webdriver")]
    {
        builder = builder.plugin(tauri_plugin_webdriver::init());
        log::info!("WebDriver plugin enabled for E2E testing");
    }

    // Single-instance: only in release builds so dev and prod can run side by side
    if !cfg!(debug_assertions) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }

    builder
        .manage(states.config.clone())
        .manage(states.pty.clone())
        .manage(states.mux.clone())
        .manage(states.notif.clone())
        .manage(states.session.clone())
        .manage(states.output_buffer.clone())
        .manage(states.screen_state.clone())
        .manage(states.agent_tracker.clone())
        .manage(states.report_store.clone())
        .manage(states.event_broadcaster.clone())
        .manage(states.update.clone())
        .manage(states.mesh.clone())
        .manage(states.keepawake.clone())
        // ServiceState is added in .setup() after AppHandle is available
        .invoke_handler(tauri::generate_handler![
            // PTY commands
            commands::get_windows_build_number,
            commands::get_default_cwd,
            commands::spawn_pty,
            commands::read_pty,
            commands::write_pty,
            commands::write_pty_string,
            commands::get_pane_output,
            commands::get_pane_text,
            commands::list_agent_providers,
            commands::wait_shell_ready,
            commands::read_session_jsonl,
            commands::resize_pty,
            commands::kill_pty,
            commands::get_pane_git_info,
            commands::suggest_install_pwsh,
            commands::get_shell_diagnostics,
            commands::restart_app,
            commands::restart_general_agent,
            commands::get_network_info,
            commands::generate_api_token,
            commands::save_clipboard_image,
            commands::save_clipboard_file,
            clipboard_files::read_clipboard_files,
            commands::save_temp_file,
            commands::copy_image_to_clipboard,
            commands::install_project_rules,
            commands::check_rule_installed,
            commands::check_for_update,
            commands::install_app_update,
            commands::open_folder,
            commands::open_url,
            commands::test_notification,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            // Multiplexer commands
            multiplexer::commands::mux_get_state,
            multiplexer::commands::mux_create_workspace,
            multiplexer::commands::mux_set_active_workspace,
            multiplexer::commands::mux_close_workspace,
            multiplexer::commands::mux_rename_workspace,
            multiplexer::commands::mux_move_tab,
            multiplexer::commands::mux_create_tab,
            multiplexer::commands::mux_close_tab,
            multiplexer::commands::mux_set_active_tab,
            multiplexer::commands::mux_close_pane,
            multiplexer::commands::mux_set_focused_pane,
            multiplexer::commands::mux_link_pty,
            multiplexer::commands::mux_rename_pane,
            multiplexer::commands::mux_update_pane_cwd,
            multiplexer::commands::mux_set_pane_agent_type,
            multiplexer::commands::mux_set_chat_view,
            multiplexer::commands::detect_claude_session_id,
            multiplexer::commands::detect_pane_session_id,
            multiplexer::commands::mux_set_claude_session_id,
            commands::check_cli_installed,
            commands::install_provider_in_pane,
            multiplexer::commands::build_agent_launch_command,
            multiplexer::commands::mux_set_pane_role,
            multiplexer::commands::mux_clear_pane_role,
            multiplexer::commands::mux_set_can_spawn,
            multiplexer::commands::mux_respond_prompt,
            multiplexer::commands::mux_batch,
            // Notification commands
            notification::notify,
            notification::list_notifications,
            notification::mark_notification_read,
            notification::clear_notifications,
            // Config commands
            config::get_config,
            config::update_config,
            config::get_theme,
            config::list_themes,
            config::get_config_path,
            config::save_custom_theme,
            config::delete_custom_theme,
            config::get_themes_dir,
            // Directory completion
            commands::list_dir_completions,
            // Claude hooks commands
            commands::check_claude_hooks,
            commands::install_claude_hooks,
            commands::uninstall_claude_hooks,
            // Session commands
            session::save_session,
            session::save_session_named,
            session::list_saved_sessions,
            session::load_session,
            // Cron commands
            commands::cron_list,
            commands::cron_get,
            commands::cron_create,
            commands::cron_update,
            commands::cron_delete,
            commands::cron_toggle,
            commands::cron_trigger,
            commands::cron_history,
            commands::cron_read_log,
            // Services commands
            commands::services_list,
            commands::services_create,
            commands::services_get,
            commands::services_update,
            commands::services_delete,
            commands::services_regenerate_token,
            commands::activity_list,
            commands::services_smart_add_inspect,
            // Mesh commands
            commands_mesh::mesh_get_status,
            commands_mesh::mesh_list_peers,
            commands_mesh::mesh_refresh_discovery,
            commands_mesh::mesh_connect,
            commands_mesh::mesh_approve_pairing,
            commands_mesh::mesh_reject_pairing,
            commands_mesh::mesh_disconnect,
            commands_mesh::mesh_exit,
            commands_mesh::mesh_add_manual,
            commands_mesh::mesh_start_discovery,
            commands_mesh::mesh_start_broadcast,
            commands_mesh::mesh_stop_broadcast,
            commands_mesh::mesh_set_access,
            // Keep-awake commands
            keepawake::keepawake_start,
            keepawake::keepawake_stop,
            keepawake::keepawake_status,
        ])
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                startup::setup_window_styling(&window);
            }

            // Create unified service context (available to both API and Tauri commands)
            let app_handle = app.handle().clone();

            // Resume any keep-awake session that was active when CodeTerm last
            // shut down. Indefinite → resume; timed + remaining → resume with
            // the remainder; timed + already expired → drop the snapshot.
            keepawake::resume_from_disk(&states.keepawake, &app_handle);

            // Bootstrap plugin system — agent-claude is a bundled native plugin
            let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
            let mut plugin_manager = plugin::PluginManager::new();
            plugin_manager.set_agent_registry(agent_registry.clone());
            plugin_manager.set_event_broadcaster(states.event_broadcaster.clone());

            // Bridge ContributionsChanged → frontend event + mem bridge rule refresh.
            {
                use tauri::Emitter;
                let mut sub = states.event_broadcaster.subscribe();
                let bridge_handle = app_handle.clone();
                let bridge_registry = agent_registry.clone();
                tauri::async_runtime::spawn(async move {
                    while let Ok(ev) = sub.recv().await {
                        if let crate::api::events::SystemEvent::ContributionsChanged { kind } = &ev {
                            let _ = bridge_handle.emit(
                                "contributions-changed",
                                serde_json::json!({ "kind": kind }),
                            );
                            if kind == "agent_provider" {
                                crate::service::sync_watcher::install_bridge_rules_for_all(
                                    &bridge_registry,
                                );
                            }
                        }
                    }
                });
            }

            // Register and activate bundled plugins
            {
                use plugin::agent_claude::AgentClaudePlugin;
                let manifest = AgentClaudePlugin::manifest();
                plugin_manager.register(manifest);
                plugin_manager.attach_native(Box::new(AgentClaudePlugin::new()))
                    .expect("agent-claude plugin attach failed");
                plugin_manager.activate(plugin::agent_claude::PLUGIN_ID)
                    .expect("agent-claude plugin activation failed");
            }
            {
                use plugin::agent_codex::AgentCodexPlugin;
                let manifest = AgentCodexPlugin::manifest();
                plugin_manager.register(manifest);
                plugin_manager.attach_native(Box::new(AgentCodexPlugin::new()))
                    .expect("agent-codex plugin attach failed");
                plugin_manager.activate(plugin::agent_codex::PLUGIN_ID)
                    .expect("agent-codex plugin activation failed");
            }
            {
                use plugin::agent_opencode::AgentOpenCodePlugin;
                let manifest = AgentOpenCodePlugin::manifest();
                plugin_manager.register(manifest);
                plugin_manager.attach_native(Box::new(AgentOpenCodePlugin::new()))
                    .expect("agent-opencode plugin attach failed");
                plugin_manager.activate(plugin::agent_opencode::PLUGIN_ID)
                    .expect("agent-opencode plugin activation failed");
            }
            log::info!("[startup] plugins activated: {:?}", plugin_manager.active());

            let prompt_detector = Arc::new(Mutex::new(agent::prompt::PromptDetector::new()));
            let (bridge_outbound_tx, bridge_outbound_rx) = bridge::manager::create_outbound_channel();

            let mem = mem::MemManager::open(&crate::paths::data_dir())
                .expect("mem manager init failed");
            match mem.seed_bundled_if_needed() {
                Ok(n) if n > 0 => log::info!("[mem] seeded {} bundled starter items", n),
                Ok(_) => log::info!("[mem] starter items already seeded"),
                Err(e) => log::warn!("[mem] seed failed: {}", e),
            }

            // Sync identity.md + soul.md into mem for search/vectorization
            crate::service::general_agent::sync_identity_to_mem(&mem);

            // Re-index any items missing vectors (catches failed bg indexing)
            mem.reindex_missing();

            // Auto-install Claude hooks (Notification, Stop, PreCompact).
            // PreCompact is critical for mem — it flushes pane scrollback
            // before Claude summarizes the context window.
            commands::ensure_claude_hooks_installed();

            let mem_supervisor = Arc::new(crate::mem::supervisor::Supervisor::new(
                crate::mem::supervisor::Supervisor::resolve_cli_path(&crate::paths::data_dir()),
                mem.db_path().to_path_buf(),
            ));

            let svc_ctx = ServiceContext {
                mux: states.mux.clone(),
                pty: states.pty.clone(),
                output_buf: states.output_buffer.clone(),
                screen_state: states.screen_state.clone(),
                notif: states.notif.clone(),
                events: states.event_broadcaster.clone(),
                app_handle: app_handle.clone(),
                agent_tracker: states.agent_tracker.clone(),
                report_store: states.report_store.clone(),
                agent_registry,
                mem,
                mem_supervisor,
                activity: crate::mem::activity::ActivityRing::new(500),
                workers: crate::mem::workers::GraphWorkerRegistry::load(
                    crate::paths::data_dir().join("mem").join("workers.json"),
                ),
                worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(
                    crate::config::memory_config().workers_per_provider,
                )),
                prompt_detector,
                agent_run_pty_map: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
                bridge_outbound: bridge_outbound_tx,
                bridge_outbound_rx: Arc::new(Mutex::new(Some(bridge_outbound_rx))),
                bridge_supervisor: Arc::new(bridge::supervisor::BridgeSupervisor::new()),
                restart_tracker: crate::service::agent_restart::new_restart_tracker(),
                secrets: {
                    let cfg_guard = states.config.lock();
                    let cfg = cfg_guard.integrations.secrets.clone();
                    drop(cfg_guard);
                    crate::secrets::new_state(&cfg)
                },
                mux_emit_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                services_store: Arc::new(
                    crate::services::store::ServicesStore::open(
                        &crate::paths::data_dir().join("services.db"),
                    )
                    .expect("services.db init"),
                ),
                tokens: Arc::new(crate::services::tokens::TokenStore::new()),
            };
            let service_state: ServiceState = Arc::new(svc_ctx);
            app.manage(service_state.clone());

            {
                let store = service_state.services_store.clone();
                let activity = crate::services::activity::ActivityWriter::new(store.conn());
                crate::services::health::scheduler::start(store, activity, app.handle().clone());
            }

            // SecretStore::init() runs inside start_servers BEFORE the
            // API listener binds — see start_servers below. That's the
            // single race-free seam for "wait until secrets are ready".

            // Librarian: hourly file-sync + skill-folder scan, run as a
            // subprocess via the supervisor so a panic / OOM cannot take
            // the daemon down. First fire after a 30s warm-up.
            {
                let supervisor = service_state.mem_supervisor.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    loop {
                        match supervisor.run(crate::mem::supervisor::WorkerKind::LibrarianOnce).await {
                            Ok(report) => log::info!("[librarian] {}", report),
                            Err(e) => log::warn!("[librarian] {}", e),
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    }
                });
            }

            // Resume extraction for items that were saved but never finished
            // (previous run crashed mid-extraction, LLM API errored, bulk import).
            // run_extract_pending returns fast after spawning its tokio task.
            {
                let svc_for_extract = service_state.clone();
                let queued = crate::mem::auto_extract::run_extract_pending(
                    svc_for_extract,
                    usize::MAX,
                );
                if queued > 0 {
                    log::info!(
                        "[mem] startup: resuming extraction for {} pending item(s)",
                        queued
                    );
                }
            }

            startup::install_bundled_cli(app);
            startup::start_servers(app, &states, &service_state);
            startup::build_system_tray(app)?;
            service::sync_watcher::start(crate::paths::data_dir(), service_state.agent_registry.clone());
            startup::start_background_tasks(&app_handle, &states);

            // PTY read loops are started by spawn_pty command (initiated by frontend)
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Only handle main window, not notification popups
                if window.label() != "main" { return; }

                // Save session
                let app = window.app_handle();
                if let (Some(mux), Some(session)) = (
                    app.try_state::<MuxState>(),
                    app.try_state::<SessionState>(),
                ) {
                    let snapshot = mux.lock().to_session_snapshot();
                    if let Err(e) = session.lock().save(&snapshot) {
                        log::error!("Failed to save session: {}", e);
                    } else {
                        log::info!("Session saved (close-to-tray)");
                    }
                }

                // Hide to tray instead of quitting
                api.prevent_close();
                let _ = window.hide();
                log::info!("Window hidden to tray");
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = _event {
                if !has_visible_windows {
                    if let Some(w) = _app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }

            // On any clean process exit (tray-Quit, Cmd+Q after window close,
            // SIGTERM caught by Tauri), checkpoint+truncate the mem WAL so a
            // stale ~4MB log doesn't survive into the next run. The updater's
            // graceful_shutdown already does this on the in-place upgrade
            // path; this catches every other shutdown route.
            if let tauri::RunEvent::Exit = _event {
                if let Some(svc) = _app.try_state::<service::ServiceState>() {
                    match svc.mem.checkpoint_and_close() {
                        Ok((busy, log_frames, checkpointed)) => log::info!(
                            "[shutdown] mem WAL checkpointed busy={} log_frames={} checkpointed={}",
                            busy, log_frames, checkpointed
                        ),
                        Err(e) => log::warn!("[shutdown] mem WAL checkpoint failed: {}", e),
                    }
                }
            }
        });
}
