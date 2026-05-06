//! Start socket server, REST API server, cron scheduler, GA, mesh, bridge.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use crate::service::ServiceState;
use crate::{api, cron, mem, mesh, service, socket, update, CronState, CronWake};

use super::states::AppStates;

/// Start socket server, REST API server, and cron scheduler.
pub(crate) fn start_servers(
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

    // Auto-install bundled mem dreaming cron if not present. Idempotent
    // by name. Schedule "0 3 * * *" + ±90min jitter (anti-anti-cheat).
    {
        let mut cs = cron_state.lock();
        match mem::dreaming::ensure_dreaming_cron(&mut cs) {
            Ok(true) => log::info!("[mem] auto-installed dreaming cron"),
            Ok(false) => log::debug!("[mem] dreaming cron already present"),
            Err(e) => log::warn!("[mem] failed to install dreaming cron: {}", e),
        }
        // Librarian now runs as a CLI subprocess via the mem supervisor.
        // Remove the legacy LLM-based cron from any older install so we don't
        // keep burning tokens to wrap a single bash command.
        if mem::dreaming::remove_legacy_librarian_cron(&mut cs) {
            log::info!("[mem] removed legacy librarian cron (replaced by direct loop)");
        }
    }

    app.manage(cron_state.clone());
    app.manage(cron_wake.clone());
    cron::scheduler::start(cron_state.clone(), cron_wake.clone(), service_state.clone());

    {
        let svc = service_state.clone();
        std::thread::spawn(move || loop {
            let cfg = crate::config::memory_config();
            let sleep_secs = if cfg.file_sync_enabled {
                cfg.file_sync_interval_minutes.max(5) * 60
            } else {
                300
            };
            std::thread::sleep(std::time::Duration::from_secs(sleep_secs));
            if crate::config::memory_config().file_sync_enabled {
                if let Err(e) = mem::file_sync::sync_all(&svc.mem) {
                    log::warn!("[file-sync] {}", e);
                }
            }
        });
    }

    // Start REST API server in its own thread with Tokio runtime
    {
        let api_config = states.config.lock().api.clone();
        if api_config.enabled {
            let api_state = api::routes::AppState {
                svc: service_state.clone(),
                cron: cron_state.clone(),
                cron_wake: cron_wake.clone(),
                api_port: api_config.port,
                update_state: states.update.clone(),
                config: states.config.clone(),
                mesh: states.mesh.clone(),
                keepawake: states.keepawake.clone(),
            };
            let router = api::routes::create_router(api_state);
            let host = api_config.host.clone();
            let port = api_config.port;
            // Set CODETERM_API_PORT so PTY children (and Claude hooks) know the correct port
            std::env::set_var("CODETERM_API_PORT", port.to_string());
            // Capture the secrets handle so we can run any one-time
            // async init (e.g. Bitwarden auto-unlock from stored master
            // password) BEFORE the listener accepts connections. Without
            // this gate, /api/secrets/status would briefly serve a stale
            // "Locked" while init was still running on a parallel task.
            let secrets_for_init = service_state.secrets.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(4)
                    .enable_all()
                    .build()
                    .expect("Failed to create Tokio runtime for REST API");
                rt.block_on(async move {
                    let init_started = std::time::Instant::now();
                    if let Err(e) = secrets_for_init.read().await.init().await {
                        log::warn!("[secrets] init failed: {}", e);
                    } else {
                        log::info!(
                            "[secrets] init done in {}ms",
                            init_started.elapsed().as_millis()
                        );
                    }
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

    // Start General Agent PTY + launch agent (workspace/pane already created in init_states)
    {
        let config = states.config.lock().general_agent.clone();
        if config.enabled {
            let svc = service_state.clone();
            let mux = states.mux.clone();
            let pty = states.pty.clone();
            let screen_state = states.screen_state.clone();
            let config_state = states.config.clone();
            let app_h = app_handle.clone();
            // Watchdog: monitors General Agent, restarts if it exits
            let w_svc = svc.clone();
            let w_mux = mux.clone();
            let w_pty = pty.clone();
            let w_screen = screen_state.clone();
            let w_config = config_state.clone();
            let w_app = app_h.clone();

            std::thread::spawn(move || {
                // Wait for frontend to initialize (workspace already created pre-lock in init_states)
                std::thread::sleep(std::time::Duration::from_secs(2));
                log::info!("[general-agent] Delayed start beginning...");
                service::general_agent::start_general_agent(
                    &svc, &mux, &pty, &screen_state, &config_state, &app_h,
                );
            });

            // Start watchdog on separate thread (waits 30s before first check)
            service::general_agent::start_watchdog(
                &w_svc, &w_mux, &w_pty, &w_screen, &w_config, &w_app,
            );
        }
    }

    // Start mesh listeners if enabled (no broadcasting — user triggers that manually)
    {
        let mesh_config = states.config.lock().mesh.clone();
        if mesh_config.enabled {
            let mesh = states.mesh.clone();
            mesh::discovery::detect_tailscale(&mesh);
            mesh::discovery::detect_local_ip(&mesh);
            if let Err(e) = mesh::discovery::start_listeners(mesh) {
                log::error!("[mesh] Listener start failed: {}", e);
            }
        }
    }

    // Start messaging bridge (Telegram, etc.) if configured
    let integrations = states.config.lock().integrations.clone();
    if integrations.telegram_enabled {
        super::bridge::spawn_telegram_bridge(service_state.clone());
    }

    // If we just booted after a self-update, push the result through the
    // bridge and clean up the staging file. Spawned task — non-blocking.
    update::boot_pickup::spawn(service_state.clone());
}
