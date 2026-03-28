mod agent;
mod api;
mod commands;
mod config;
pub mod cron;
pub mod multiplexer;
mod notification;
pub mod paths;
pub mod prompts;
pub mod pty;
mod screenshot;
pub mod service;
pub mod shell;
pub mod session;
mod socket;
pub mod update;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use config::ConfigState;
use multiplexer::commands::MuxState;
use multiplexer::state::MultiplexerState;
use notification::NotifState;
use notification::store::NotificationStore;
use pty::manager::{OutputBuffer, PtyManager};
use screenshot::{ScreenshotChannel, ScreenshotState};
use service::{ServiceContext, ServiceState};
use session::SessionState;
use session::store::SessionStore;

pub type PtyState = Arc<Mutex<PtyManager>>;
pub type OutputBufferState = Arc<Mutex<OutputBuffer>>;
pub type AgentTrackerState = Arc<Mutex<agent::status::PaneStatusTracker>>;
pub type ReportStoreState = Arc<Mutex<agent::reports::ReportStore>>;
pub use cron::scheduler::{CronState, CronWake};
pub type UpdateStateHandle = Arc<Mutex<update::UpdateState>>;

/// All shared application states, grouped for easier passing between init functions.
struct AppStates {
    config: ConfigState,
    pty: PtyState,
    mux: MuxState,
    notif: NotifState,
    session: SessionState,
    output_buffer: OutputBufferState,
    screenshot: ScreenshotState,
    agent_tracker: AgentTrackerState,
    report_store: ReportStoreState,
    event_broadcaster: api::events::EventBroadcasterState,
    update: UpdateStateHandle,
}

/// Create and initialize all application states.
fn init_states() -> AppStates {
    let app_config = config::load_config();
    log::info!("Config loaded: theme={}", app_config.theme.name);
    let config_state: ConfigState = Arc::new(Mutex::new(app_config));

    let pty_state: PtyState = Arc::new(Mutex::new(PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::new(500)));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState = Arc::new(Mutex::new(OutputBuffer::new(65536)));
    let screenshot_state: ScreenshotState = Arc::new(Mutex::new(ScreenshotChannel::new()));
    let agent_tracker: AgentTrackerState = Arc::new(Mutex::new(agent::status::PaneStatusTracker::new()));
    let report_store: ReportStoreState = Arc::new(Mutex::new(agent::reports::ReportStore::new(200)));
    let event_broadcaster: api::events::EventBroadcasterState = Arc::new(api::events::EventBroadcaster::new());

    // Detect install method and init update state
    let install_method = update::detect_install_method();
    log::info!("Install method detected: {}", install_method);
    let update_state: UpdateStateHandle = Arc::new(Mutex::new(update::UpdateState {
        install_method,
        last_check: None,
        last_check_time: None,
    }));

    // Try to restore last session, fallback to default workspace
    let mux_state_inner = {
        let last = session_state.lock().load_last();
        match last {
            Ok(Some(snapshot)) if !snapshot.workspaces.is_empty() => {
                log::info!("Restoring session with {} workspaces, {} tabs total",
                    snapshot.workspaces.len(),
                    snapshot.workspaces.iter().map(|w| w.tabs.len()).sum::<usize>());
                for ws in &snapshot.workspaces {
                    log::info!("  Workspace '{}': {} tabs, active_tab={:?}",
                        ws.name, ws.tabs.len(), ws.active_tab_id);
                }
                MultiplexerState::from_session_snapshot(&snapshot)
            }
            _ => {
                log::info!("No saved session, creating default workspace");
                let (state, _) = MultiplexerState::new();
                state
            }
        }
    };
    let mux_state: MuxState = Arc::new(Mutex::new(mux_state_inner));

    AppStates {
        config: config_state,
        pty: pty_state,
        mux: mux_state,
        notif: notif_state,
        session: session_state,
        output_buffer,
        screenshot: screenshot_state,
        agent_tracker,
        report_store,
        event_broadcaster,
        update: update_state,
    }
}

/// Set window icon and apply platform-specific styling.
fn setup_window_styling(window: &tauri::WebviewWindow) {
    let icon_bytes = include_bytes!("../icons/icon.png");
    if let Ok(mut reader) = png::Decoder::new(std::io::Cursor::new(icon_bytes)).read_info() {
        let info = reader.info().clone();
        let mut rgba = vec![0u8; reader.output_buffer_size()];
        if reader.next_frame(&mut rgba).is_ok() {
            let icon = tauri::image::Image::new_owned(rgba, info.width, info.height);
            let _ = window.set_icon(icon);
        }
    }

    // macOS: rounded corners for frameless window
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowStyleMask, NSWindowTitleVisibility};
        let ns_win = window.ns_window().unwrap() as *mut objc2::runtime::AnyObject;
        unsafe {
            let ns_window: &NSWindow = &*(ns_win as *const NSWindow);
            let mut mask = ns_window.styleMask();
            mask |= NSWindowStyleMask::Titled;
            mask |= NSWindowStyleMask::FullSizeContentView;
            ns_window.setStyleMask(mask);
            ns_window.setTitlebarAppearsTransparent(true);
            ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            ns_window.setHasShadow(true);
            // Hide traffic light buttons
            use objc2::msg_send;
            for i in 0..3_isize {
                let btn: *mut objc2::runtime::AnyObject = msg_send![ns_window, standardWindowButton: i];
                if !btn.is_null() {
                    let _: () = msg_send![btn, setHidden: true];
                }
            }
        }
    }
}

/// Build and configure the system tray icon and menu.
fn build_system_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
    use tauri::menu::{MenuBuilder, MenuItemBuilder};

    let show = MenuItemBuilder::with_id("show", "Show CodeTerm").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&quit)
        .build()?;

    let tray_icon = {
        // macOS: template image (white on transparent, auto-adapts to light/dark)
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../icons/tray-template.png");
        #[cfg(not(target_os = "macos"))]
        let bytes = include_bytes!("../icons/32x32.png");

        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info().expect("Failed to decode tray icon PNG");
        let info = reader.info().clone();
        let mut rgba = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut rgba).expect("Failed to read tray icon frame");
        tauri::image::Image::new_owned(rgba, info.width, info.height)
    };

    let _tray = TrayIconBuilder::new()
        .tooltip("CodeTerm")
        .icon(tray_icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "show" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

/// Start background threads: auto-save, update check, agent idle monitoring.
fn start_background_tasks(
    app_handle: &tauri::AppHandle,
    states: &AppStates,
) {
    // Auto-save session every 30 seconds
    {
        let app_handle = app_handle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(30));
                if let (Some(mux), Some(session)) = (
                    app_handle.try_state::<MuxState>(),
                    app_handle.try_state::<SessionState>(),
                ) {
                    let snapshot = mux.lock().to_session_snapshot();
                    if let Err(e) = session.lock().save(&snapshot) {
                        log::warn!("Auto-save failed: {}", e);
                    }
                }
            }
        });
    }

    // Background update check (5s delay to not slow startup)
    {
        let update_st = states.update.clone();
        let app_h = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create update check runtime");
            rt.block_on(async {
                let method = update_st.lock().install_method.clone();
                match crate::update::checker::check_for_update(&method).await {
                    Ok(info) => {
                        if info.update_available {
                            log::info!(
                                "Update available: {} → {}",
                                info.current_version,
                                info.latest_version
                            );
                            use tauri::Emitter;
                            let _ = app_h.emit(
                                "update-available",
                                serde_json::to_value(&info).unwrap_or_default(),
                            );
                        }
                        let mut us = update_st.lock();
                        us.last_check = Some(info);
                        us.last_check_time = Some(std::time::Instant::now());
                    }
                    Err(e) => log::warn!("Update check failed: {}", e),
                }
            });
        });
    }

    // Agent idle check loop — transitions Working -> Idle after timeout
    {
        let tracker = states.agent_tracker.clone();
        let mux = states.mux.clone();
        let idle_timeout = {
            let cfg = states.config.lock();
            std::time::Duration::from_secs(cfg.agent.idle_timeout_s as u64)
        };
        let poll_interval = {
            let cfg = states.config.lock();
            std::time::Duration::from_millis(cfg.agent.poll_interval_ms as u64)
        };
        let app_handle = app_handle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(poll_interval);
                // Collect status changes while holding tracker lock,
                // then release before touching mux (prevents deadlock with process_pty_output)
                let changed: Vec<(String, crate::agent::status::PaneStatus)> = {
                    let mut t = tracker.lock();
                    let pty_ids = t.all_ids();
                    let mut changes = Vec::new();
                    for pty_id in pty_ids {
                        let prev = t.get_status(&pty_id);
                        t.check_idle(&pty_id, idle_timeout);
                        let now = t.get_status(&pty_id);
                        if now != prev {
                            changes.push((pty_id, now));
                        }
                    }
                    changes
                }; // tracker lock released here

                for (pty_id, status) in changed {
                    let pane_id = mux.lock().pane_id_for_pty(&pty_id);
                    if let Some(pid) = pane_id {
                        use tauri::Emitter;
                        let _ = app_handle.emit("pane-status-changed", serde_json::json!({
                            "pane_id": pid,
                            "status": format!("{:?}", status),
                        }));
                    }
                }
            }
        });
    }
}

/// Start socket server, REST API server, and cron scheduler.
fn start_servers(
    app: &tauri::App,
    states: &AppStates,
    service_state: &ServiceState,
) {
    let app_handle = app.handle().clone();

    // Start socket server
    if let Err(e) = socket::server::SocketServer::start(
        states.mux.clone(),
        states.pty.clone(),
        states.notif.clone(),
        app_handle.clone(),
        states.output_buffer.clone(),
        service_state.clone(),
    ) {
        log::error!("Failed to start socket server: {}", e);
    }

    // Initialize cron store and scheduler
    let cron_store = cron::store::CronStore::new();
    let cron_state: CronState = Arc::new(Mutex::new(cron_store));
    let cron_wake: CronWake = Arc::new(std::sync::Condvar::new());
    app.manage(cron_state.clone());
    app.manage(cron_wake.clone());
    cron::scheduler::start(cron_state.clone(), cron_wake.clone(), service_state.clone());

    // Start REST API server in its own thread with Tokio runtime
    {
        let api_config = states.config.lock().api.clone();
        if api_config.enabled {
            let api_state = api::routes::AppState {
                svc: service_state.clone(),
                token: api_config.token.clone(),
                screenshot: states.screenshot.clone(),
                cron: cron_state.clone(),
                cron_wake: cron_wake.clone(),
                api_port: api_config.port,
                update_state: states.update.clone(),
            };
            let router = api::routes::create_router(api_state);
            let host = api_config.host.clone();
            let port = api_config.port;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(4)
                    .enable_all()
                    .build()
                    .expect("Failed to create Tokio runtime for REST API");
                rt.block_on(async move {
                    let bind_addr = format!("{}:{}", host, port);
                    match tokio::net::TcpListener::bind(&bind_addr).await {
                        Ok(listener) => {
                            log::info!("REST API listening on http://{}", bind_addr);
                            if let Err(e) = axum::serve(listener, router).await {
                                log::error!("REST API server error: {}", e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to bind REST API on port {}: {}", port, e);
                        }
                    }
                });
            });
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    log::info!("CodeTerm starting...");

    // Write shell integration scripts (OSC 7 CWD tracking)
    pty::shell_integration::ensure_scripts();

    // Initialize all shared states
    let states = init_states();

    // Auto-update installed skills if bundled version is newer
    let updated_skills = commands::auto_update_skills();
    if !updated_skills.is_empty() {
        log::info!("Auto-updated skills: {}", updated_skills.join(", "));
    }

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

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
        .manage(states.agent_tracker.clone())
        .manage(states.report_store.clone())
        .manage(states.event_broadcaster.clone())
        .manage(states.screenshot.clone())
        .manage(states.update.clone())
        // ServiceState is added in .setup() after AppHandle is available
        .invoke_handler(tauri::generate_handler![
            // PTY commands
            commands::get_default_cwd,
            commands::spawn_pty,
            commands::read_pty,
            commands::write_pty,
            commands::write_pty_string,
            commands::get_pane_output,
            commands::read_session_jsonl,
            commands::resize_pty,
            commands::kill_pty,
            commands::get_pane_git_info,
            commands::list_skills,
            commands::install_skill,
            commands::uninstall_skill,
            commands::get_skills_prompt,
            commands::get_network_info,
            commands::generate_api_token,
            commands::save_clipboard_image,
            commands::save_temp_file,
            commands::copy_image_to_clipboard,
            commands::install_project_rules,
            commands::check_rule_installed,
            commands::check_for_update,
            commands::install_app_update,
            commands::open_folder,
            commands::open_url,
            commands::test_notification,
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
            multiplexer::commands::mux_split_pane,
            multiplexer::commands::mux_close_pane,
            multiplexer::commands::mux_set_focused_pane,
            multiplexer::commands::mux_link_pty,
            multiplexer::commands::mux_rename_pane,
            multiplexer::commands::mux_update_pane_cwd,
            multiplexer::commands::mux_set_pane_agent_type,
            multiplexer::commands::mux_set_chat_view,
            multiplexer::commands::detect_claude_session_id,
            multiplexer::commands::mux_set_claude_session_id,
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
            // Screenshot commands
            screenshot::capture_screenshot,
            screenshot::submit_screenshot,
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
        ])
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                setup_window_styling(&window);
            }

            // Create unified service context (available to both API and Tauri commands)
            let app_handle = app.handle().clone();
            let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
            let svc_ctx = ServiceContext {
                mux: states.mux.clone(),
                pty: states.pty.clone(),
                output_buf: states.output_buffer.clone(),
                notif: states.notif.clone(),
                events: states.event_broadcaster.clone(),
                app_handle: app_handle.clone(),
                agent_tracker: states.agent_tracker.clone(),
                report_store: states.report_store.clone(),
                agent_registry,
            };
            let service_state: ServiceState = Arc::new(svc_ctx);
            app.manage(service_state.clone());

            start_servers(app, &states, &service_state);
            build_system_tray(app)?;
            start_background_tasks(&app_handle, &states);

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
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = event {
                if !has_visible_windows {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        });
}
