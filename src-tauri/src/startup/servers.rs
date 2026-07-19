//! Start socket server, REST API server, cron scheduler, GA, mesh, bridge.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use crate::service::ServiceState;
use crate::{api, cron, mem, mesh, service, socket, update, CronState, CronWake};

use super::states::AppStates;

/// Start socket server, REST API server, and cron scheduler.
pub(crate) fn start_servers(app: &tauri::App, states: &AppStates, service_state: &ServiceState) {
    let app_handle = app.handle().clone();
    let frontend_readiness_generation = super::arm_frontend_readiness();

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

    // e2e suite can time-travel; release builds never do.
    let test_clock: Option<Arc<cron::clock::MockClock>> =
        if cfg!(debug_assertions) && std::env::var("CODETERM_TEST_CLOCK").as_deref() == Ok("1") {
            log::warn!("[cron] CODETERM_TEST_CLOCK=1 — cron runs on a mock clock");
            Some(cron::clock::MockClock::new())
        } else {
            None
        };
    let cron_store = match &test_clock {
        Some(clock) => cron::store::CronStore::new_with_clock(clock.clone()),
        None => cron::store::CronStore::new(),
    };
    let cron_state: CronState = Arc::new(Mutex::new(cron_store));
    let cron_wake: CronWake = Arc::new(std::sync::Condvar::new());

    // Auto-install bundled mem dreaming cron if not present. Idempotent by name. Schedule "0 3 * * *" + ±90min jitter (anti-anti-cheat).
    {
        let mut cs = cron_state.lock();
        let mem_cfg = crate::config::memory_config();
        let default_agent = crate::config::default_agent_provider();
        match mem::dreaming::ensure_dreaming_cron(
            &mut crate::mem_host::CronSchedulerPort(&mut cs),
            &mem_cfg,
            &default_agent,
        ) {
            Ok(true) => log::info!("[mem] auto-installed dreaming cron"),
            Ok(false) => log::debug!("[mem] dreaming cron already present"),
            Err(e) => log::warn!("[mem] failed to install dreaming cron: {}", e),
        }
        // burning tokens to wrap a single bash command.
        if mem::dreaming::remove_legacy_librarian_cron(&mut crate::mem_host::CronSchedulerPort(
            &mut cs,
        )) {
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
                resize_owner: states.resize_owner.clone(),
                test_clock: test_clock.clone(),
                events: states.event_broadcaster.clone(),
                web_dist: resolve_web_dist(),
            };
            let router = api::routes::create_router(api_state);
            let host = api_config.host.clone();
            let port = api_config.port;
            if host == "0.0.0.0" {
                log::warn!(
                    "[api] binding to 0.0.0.0 exposes the API to all network interfaces — \
                     consider setting api.host = \"127.0.0.1\" in config for better security"
                );
            }
            std::env::set_var("CODETERM_API_PORT", port.to_string());
            // accepts connections. Without this gate, /api/secrets/status would briefly serve a stale "Locked" while init was still running on a parallel task.
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
        if config.enabled && config.first_run_complete {
            let svc = service_state.clone();
            let mux = states.mux.clone();
            let pty = states.pty.clone();
            let screen_state = states.screen_state.clone();
            let config_state = states.config.clone();
            let app_h = app_handle.clone();
            let w_svc = svc.clone();
            let w_mux = mux.clone();
            let w_pty = pty.clone();
            let w_screen = screen_state.clone();
            let w_config = config_state.clone();
            let w_app = app_h.clone();
            let frontend_ready_deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(30);

            std::thread::spawn(move || {
                if !super::wait_for_frontend_ready_until(
                    frontend_readiness_generation,
                    frontend_ready_deadline,
                ) {
                    let reason = "frontend did not report readiness before the launch deadline";
                    log::error!("[general-agent] {reason}");
                    if let Some(pane_id) = mux
                        .write()
                        .resolve_pane_id(crate::service::general_agent::GENERAL_ALIAS)
                    {
                        svc.emit_boot_stage(
                            &pane_id,
                            codeterm_types::pty::BootStage::SpawnFailed,
                            Some(reason),
                        );
                    }
                    return;
                }
                log::info!("[general-agent] Startup beginning...");
                service::general_agent::start_general_agent(
                    &svc,
                    &mux,
                    &pty,
                    &screen_state,
                    &config_state,
                    &app_h,
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

    let integrations = states.config.lock().integrations.clone();
    if integrations.telegram_enabled {
        super::bridge::spawn_telegram_bridge(service_state.clone());
    }

    // If the previous instance was killed by `update --apply --after-update`, pick up the queued message and inject it into the general pane once the agent is ready.
    update::post_update::spawn(service_state.clone());
}

/// Locate the built React bundle for the browser mirror.
/// Priority: explicit env override → repo dist (dev builds) →
/// exe-adjacent dist (portable/prod layouts). None → the browser UI returns 404
/// and the daemon logs the miss once.
fn resolve_web_dist() -> Option<std::path::PathBuf> {
    let candidates = [
        std::env::var("CODETERM_WEB_DIST")
            .ok()
            .map(std::path::PathBuf::from),
        Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("dist"),
        ),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("dist"))),
        // macOS .app: exe is Contents/MacOS/<bin>, bundled resources land in Contents/Resources/.
        std::env::current_exe().ok().and_then(|e| {
            e.parent()
                .and_then(|d| d.parent())
                .map(|d| d.join("Resources").join("dist"))
        }),
        // Tauri layouts that nest bundled resources under a `resources/` subdir.
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("resources").join("dist"))),
    ];
    for cand in candidates.into_iter().flatten() {
        if cand.join("index.html").exists() {
            log::info!("[web] serving browser UI from {}", cand.display());
            return Some(cand);
        }
    }
    log::warn!("[web] no dist/ bundle found — browser UI disabled (build with `pnpm build`)");
    None
}
