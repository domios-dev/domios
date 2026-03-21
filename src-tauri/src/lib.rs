mod agent;
mod api;
mod commands;
mod config;
pub mod multiplexer;
mod notification;
pub mod pty;
mod screenshot;
pub mod session;
mod socket;

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
use session::SessionState;
use session::store::SessionStore;

pub type PtyState = Arc<Mutex<PtyManager>>;
pub type OutputBufferState = Arc<Mutex<OutputBuffer>>;
pub type AgentTrackerState = Arc<Mutex<agent::status::PaneStatusTracker>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    log::info!("CodeTerm starting...");

    // Load configuration
    let app_config = config::load_config();
    log::info!("Config loaded: theme={}", app_config.theme.name);
    let config_state: ConfigState = Arc::new(Mutex::new(app_config));

    let pty_state: PtyState = Arc::new(Mutex::new(PtyManager::new()));
    let notif_state: NotifState = Arc::new(Mutex::new(NotificationStore::new(500)));
    let session_state: SessionState = Arc::new(Mutex::new(SessionStore::new()));
    let output_buffer: OutputBufferState = Arc::new(Mutex::new(OutputBuffer::new(65536)));
    let screenshot_state: ScreenshotState = Arc::new(Mutex::new(ScreenshotChannel::new()));
    let agent_tracker: AgentTrackerState = Arc::new(Mutex::new(agent::status::PaneStatusTracker::new()));
    let event_broadcaster: api::events::EventBroadcasterState = Arc::new(api::events::EventBroadcaster::new());

    // Try to restore last session, fallback to default workspace
    let mux_state_inner = {
        let last = session_state.lock().load_last();
        match last {
            Ok(Some(snapshot)) if !snapshot.workspaces.is_empty() => {
                log::info!("Restoring session with {} workspaces", snapshot.workspaces.len());
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

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(config_state.clone())
        .manage(pty_state.clone())
        .manage(mux_state.clone())
        .manage(notif_state.clone())
        .manage(session_state.clone())
        .manage(output_buffer.clone())
        .manage(agent_tracker.clone())
        .manage(event_broadcaster.clone())
        .manage(screenshot_state.clone())
        .invoke_handler(tauri::generate_handler![
            // PTY commands
            commands::get_default_cwd,
            commands::spawn_pty,
            commands::read_pty,
            commands::write_pty,
            commands::write_pty_string,
            commands::resize_pty,
            commands::kill_pty,
            commands::get_pane_output,
            commands::get_pane_git_info,
            commands::list_skills,
            commands::install_skill,
            commands::uninstall_skill,
            commands::get_skills_prompt,
            commands::get_network_info,
            commands::save_clipboard_image,
            commands::copy_image_to_clipboard,
            commands::install_project_rules,
            commands::check_rule_installed,
            commands::open_folder,
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
            multiplexer::commands::mux_update_pane_cwd,
            multiplexer::commands::mux_set_pane_agent_type,
            multiplexer::commands::detect_claude_session_id,
            multiplexer::commands::mux_set_claude_session_id,
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
        ])
        .setup(move |app| {
            // Set window icon (decode PNG → RGBA for Tauri)
            if let Some(window) = app.get_webview_window("main") {
                let icon_bytes = include_bytes!("../icons/icon.png");
                if let Ok(mut reader) = png::Decoder::new(std::io::Cursor::new(icon_bytes)).read_info() {
                    let info = reader.info().clone();
                    let mut rgba = vec![0u8; reader.output_buffer_size()];
                    if reader.next_frame(&mut rgba).is_ok() {
                        let icon = tauri::image::Image::new_owned(rgba, info.width, info.height);
                        let _ = window.set_icon(icon);
                    }
                }
            }

            // Start socket server
            let app_handle = app.handle().clone();
            if let Err(e) = socket::server::SocketServer::start(
                mux_state.clone(),
                pty_state.clone(),
                notif_state.clone(),
                app_handle.clone(),
                output_buffer.clone(),
            ) {
                log::error!("Failed to start socket server: {}", e);
            }

            // Start REST API server in its own thread with Tokio runtime
            {
                let api_config = config_state.lock().api.clone();
                if api_config.enabled {
                    let api_state = api::routes::AppState {
                        mux: mux_state.clone(),
                        pty: pty_state.clone(),
                        notif: notif_state.clone(),
                        output_buf: output_buffer.clone(),
                        screenshot: screenshot_state.clone(),
                        events: event_broadcaster.clone(),
                        app_handle: app_handle.clone(),
                        token: api_config.token.clone(),
                    };
                    let router = api::routes::create_router(api_state);
                    let host = api_config.host.clone();
                    let port = api_config.port;
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
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

            // System tray
            {
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
            }

            // Auto-save session every 30 seconds
            {
                let app_handle = app.handle().clone();
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

            // PTY read loops are started by spawn_pty command (initiated by frontend)
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Only save session for main window, not notification popups
                if window.label() != "main" { return; }
                let app = window.app_handle();
                if let (Some(mux), Some(session)) = (
                    app.try_state::<MuxState>(),
                    app.try_state::<SessionState>(),
                ) {
                    let snapshot = mux.lock().to_session_snapshot();
                    if let Err(e) = session.lock().save(&snapshot) {
                        log::error!("Failed to save session: {}", e);
                    } else {
                        log::info!("Session saved on close");
                    }
                }

                // Let the window close normally
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
