//! Start socket server, REST API server, cron scheduler, GA, mesh, bridge.

use codeterm_service_core::service::MuxEmitCause;
use std::sync::Arc;

use parking_lot::Mutex;

use codeterm_service_core::multiplexer::types::TabBacking;
use codeterm_service_core::service::ServiceState;
use codeterm_service_core::{cron, mem, mesh, service, CronState, CronWake};

use codeterm_service_core::startup::AppStates;

fn should_materialize_restored_backend(
    restored_from_snapshot: bool,
    protected: bool,
    backing: &TabBacking,
    pty_id: Option<&str>,
    frozen: bool,
) -> bool {
    restored_from_snapshot
        && !protected
        && !frozen
        && !matches!(backing, TabBacking::Surface(_))
        && pty_id.is_none()
}

fn start_restored_pty_backends(
    service_state: ServiceState,
    initialized: std::sync::mpsc::Receiver<()>,
) {
    codeterm_service_core::startup::boot_timing::mark("-", None, "restore_scheduled");
    codeterm_util::named_thread::spawn("restore-pty-backends", move || {
        if !service_initialized(initialized) {
            log::error!("[startup-pty] service initialization acknowledgement unavailable");
            return;
        }
        let queued = restored_backend_tab_ids(&service_state);
        for tab_id in &queued {
            codeterm_service_core::startup::boot_timing::mark(tab_id, None, "wave_queued");
        }
        let started = std::time::Instant::now();
        log::debug!(
            "[startup-pty] wave started tabs={} pty_reserved={} pty_shared={} chat_reserved={} chat_shared={}",
            queued.len(),
            super::restored_backends::RESTORE_BUDGETS.foreground,
            super::restored_backends::RESTORE_BUDGETS.background,
            super::restored_backends::RESTORE_BUDGETS.chat_foreground,
            super::restored_backends::RESTORE_BUDGETS.chat_background,
        );
        let results = super::restored_backends::run_transport_waves(
            &queued,
            super::restored_backends::RESTORE_BUDGETS,
            |tab_id| {
                if service_state.tab_chat_backing(tab_id).is_some() {
                    super::restored_backends::RestoreTransport::Chat
                } else {
                    super::restored_backends::RestoreTransport::Pty
                }
            },
            || {
                let mux = service_state.mux.read();
                queued
                    .iter()
                    .map(|tab_id| service::restore_admission::priority(&mux, tab_id))
                    .collect()
            },
            |tab_id| {
                codeterm_service_core::startup::boot_timing::mark(tab_id, None, "cwd_check_enter");
                let cwd = service_state
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.cwd.clone());
                let result = service_state
                    .validate_request_cwd(None, Some(tab_id), cwd.as_deref())
                    .map_err(|error| error.data().to_string());
                if let Err(reason) = &result {
                    publish_backend_materialization_failure(&service_state, tab_id, reason);
                    log::error!("[startup-pty] tab={tab_id} cwd refused: {reason}");
                }
                codeterm_service_core::startup::boot_timing::mark(tab_id, None, "cwd_check_done");
                result
            },
            |tab_id, admission| {
                admission.mark(tab_id, None, "wave_admitted");
                let result = if service_state.tab_chat_backing(tab_id).is_some() {
                    service_state
                        .chatbackend_reopen_if_restored(tab_id)
                        .map(|_| ())
                } else {
                    service::restore_admission::admit_restored_pty(
                        &service_state,
                        tab_id,
                        admission,
                    )
                };
                if let Err(reason) = &result {
                    publish_backend_materialization_failure(&service_state, tab_id, reason);
                    log::error!("[startup-pty] tab={tab_id} materialization failed: {reason}");
                }
                log::debug!(
                    "[startup-pty] tab={tab_id} admitted elapsed_ms={} ok={}",
                    started.elapsed().as_millis(),
                    result.is_ok()
                );
                if service_state.tab_chat_backing(tab_id).is_some() {
                    codeterm_service_core::startup::boot_timing::mark(tab_id, None, "materialized");
                }
                result
            },
        );
        log::debug!(
            "[startup-pty] wave finished elapsed_ms={} failed={}",
            started.elapsed().as_millis(),
            results.iter().filter(|result| result.is_err()).count()
        );
    });
}

fn publish_backend_materialization_failure(service: &ServiceState, tab_id: &str, reason: &str) {
    if let Some(tab) = service.mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = false;
    }
    service.emit_boot_stage(
        tab_id,
        codeterm_types::pty::BootStage::SpawnFailed,
        Some(reason),
    );
    service.emit_state_changed(MuxEmitCause::Startup);
}

fn service_initialized(initialized: std::sync::mpsc::Receiver<()>) -> bool {
    initialized.recv().is_ok()
}

fn restored_backend_tab_ids(service_state: &ServiceState) -> Vec<String> {
    let (snapshot, general_id) = {
        let mux = service_state.mux.read();
        (
            mux.snapshot(),
            service::general_agent::general_workspace_id(&mux),
        )
    };
    let general_tabs = snapshot
        .workspaces
        .iter()
        .find(|workspace| Some(&workspace.id) == general_id.as_ref())
        .map(|workspace| {
            workspace
                .tabs
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let candidates = snapshot
        .tabs
        .into_values()
        .filter(|tab| {
            !tab.close_restore
                .as_ref()
                .is_some_and(|closed| closed.stopped)
                && should_materialize_restored_backend(
                    tab.restored_from_snapshot,
                    tab.protected,
                    &tab.backing,
                    tab.pty_id.as_deref(),
                    tab.frozen,
                )
        })
        .map(|tab| {
            let general = general_tabs.contains(&tab.id);
            (tab.id, general, tab.last_active_at_ms)
        })
        .collect();
    super::restored_backends::ordered_ids(candidates)
}

/// Start socket server, REST API server, and cron scheduler.
pub(crate) fn start_servers(
    states: &AppStates,
    service_state: &ServiceState,
    frontend_dev_url: Option<url::Url>,
    resources: Option<std::path::PathBuf>,
    quit_port: Option<codeterm_api::app_quit_routes::AppQuitPortHandle>,
) -> Result<Option<Arc<codeterm_frontend_bundle::FrontendBundles>>, String> {
    codeterm_commands::update::bind_status_emitter(service_state.ui.clone());
    super::state_incidents::install_notifier(service_state);
    if let Some(reason) = states.storage.unavailable_reason() {
        super::state_incidents::notify_storage_unavailable(
            service_state,
            &codeterm_service_core::paths::data_dir().join("state.db"),
            reason,
        );
        if let Err(error) = codeterm_service_core::service::session_persist::set_storage_unavailable(
            service_state,
            reason.to_string(),
        ) {
            log::error!("[session-persist] unavailable backend setup failed: {error}");
        }
    } else if let Some(storage) = states.storage.storage() {
        let adapter = codeterm_service_core::session::store::StorageDeskAdapter::from_storage(
            storage.clone(),
        );
        if let Err(error) =
            codeterm_service_core::service::session_persist::install_storage_desk_adapter(
                service_state,
                adapter,
            )
        {
            log::error!("[session-persist] Storage backend setup failed: {error}");
        }
    }

    if states.boot_resync && states.storage.storage().is_some() {
        let resync =
            codeterm_service_core::service::session_persist::write_session_snapshot_with_intent(
                service_state,
                &states.mux,
                codeterm_service_core::session::store::SaveIntent::AuthorizedReduction,
            );
        if let Err(error) = resync {
            log::error!("[restore] Storage desk boot resync failed: {error}");
        }
    }

    // e2e suite can time-travel; release builds never do.
    let test_clock: Option<Arc<cron::clock::MockClock>> =
        if cfg!(debug_assertions) && std::env::var("CODETERM_TEST_CLOCK").as_deref() == Ok("1") {
            log::warn!("[cron] CODETERM_TEST_CLOCK=1 — cron runs on a mock clock");
            Some(cron::clock::MockClock::new())
        } else {
            None
        };
    let cron_clock: cron::clock::SharedClock = test_clock
        .clone()
        .map(|clock| clock as cron::clock::SharedClock)
        .unwrap_or_else(cron::clock::system_clock);
    let logs_root = codeterm_service_core::paths::data_dir().join("crons");
    let default_agent_provider = states.config.lock().general_agent.agent_provider.clone();
    let cron_store = match states.storage.storage() {
        Some(storage) => cron::store::CronStore::open_storage(
            storage,
            logs_root.clone(),
            cron_clock.clone(),
            cron::store::CronStorageOptions::default(),
            default_agent_provider,
        )
        .unwrap_or_else(|error| cron::store::CronStore::unavailable(error, logs_root, cron_clock)),
        None => cron::store::CronStore::unavailable(
            states
                .storage
                .unavailable_reason()
                .unwrap_or("Storage unavailable")
                .to_string(),
            logs_root,
            cron_clock,
        ),
    };
    let cron_available = cron_store.unavailable_reason().is_none();
    let cron_state: CronState = Arc::new(Mutex::new(cron_store));
    let cron_wake: CronWake = Arc::new(std::sync::Condvar::new());

    // Auto-install bundled mem dreaming cron if not present. Idempotent by name. Schedule "0 3 * * *" + ±90min jitter (anti-anti-cheat).
    if cron_available {
        let general_workspace = service::general_agent::general_workspace_id(&states.mux.read());
        let mut cs = cron_state.lock();
        let mem_cfg = codeterm_service_core::config::memory_config();
        let default_agent = codeterm_service_core::config::default_agent_provider();
        match general_workspace
            .as_deref()
            .ok_or_else(|| "General workspace unavailable".to_string())
            .and_then(|workspace| {
                mem::dreaming::ensure_dreaming_cron(
                    &mut codeterm_service_core::mem_host::CronSchedulerPort(&mut cs),
                    &mem_cfg,
                    &default_agent,
                    &codeterm_service_core::service::spawn::CatalogBackgroundModelPort,
                    workspace,
                )
            }) {
            Ok(true) => log::info!("[mem] auto-installed dreaming cron"),
            Ok(false) => log::debug!("[mem] dreaming cron already present"),
            Err(e) => log::warn!("[mem] failed to install dreaming cron: {}", e),
        }
    }

    let port_ready = if cron_available {
        match cron::store::settle_port(&cron_state) {
            cron::store::PortSettlement::Empty | cron::store::PortSettlement::Committed => true,
            cron::store::PortSettlement::Retained(error)
            | cron::store::PortSettlement::Discarded(error)
            | cron::store::PortSettlement::Unconfirmed(error) => {
                log::error!("[cron] startup scheduler port settlement failed: {error}");
                cron_state
                    .lock()
                    .mark_scheduler_unavailable(error.to_string());
                false
            }
        }
    } else {
        false
    };
    let restored_cron_session = {
        let mux = states.mux.read();
        cron::RestoredCronSession::from_mux(&mux)
    };
    let scheduler_ready = port_ready
        && match cron::reconcile_restored_runs(&cron_state, &restored_cron_session) {
            Ok((retained, terminalized)) => {
                if retained + terminalized > 0 {
                    log::info!(
                        "[cron] startup reconciliation retained {retained} live run(s) and terminalized {terminalized} orphaned run(s)"
                    );
                }
                true
            }
            Err(error) => {
                log::error!("[cron] startup run reconciliation failed: {error}");
                cron_state
                    .lock()
                    .mark_scheduler_unavailable(error.to_string());
                false
            }
        };

    assert!(service_state.set_ext(cron_state.clone()).is_ok());
    assert!(service_state.set_ext(cron_wake.clone()).is_ok());
    let fence_wake = cron_wake.clone();
    cron_state
        .lock()
        .set_fence_notifier(move || fence_wake.notify_all());
    if scheduler_ready {
        service_state
            .start_curator_events()
            .expect("Curator runtime registration");
        cron::scheduler::start(cron_state.clone(), cron_wake.clone(), service_state.clone());
    }
    let sentinel_clock: cron::clock::SharedClock = test_clock
        .clone()
        .map(|clock| clock as cron::clock::SharedClock)
        .unwrap_or_else(cron::clock::system_clock);
    if let Some(storage) = states.storage.storage() {
        codeterm_sentinel_host::scheduler::start(
            service_state.clone(),
            sentinel_clock,
            storage,
            &codeterm_service_core::paths::data_dir(),
        );
    }

    let secrets = service_state.secrets.clone();
    codeterm_service_core::async_runtime::spawn(async move {
        let init_started = std::time::Instant::now();
        let store = secrets.read().await.clone();
        if let Err(error) = store.init().await {
            log::warn!("[secrets] init failed: {error}");
        } else {
            log::debug!(
                "[secrets] init done in {}ms",
                init_started.elapsed().as_millis()
            );
        }
    });

    {
        let svc = service_state.clone();
        codeterm_util::named_thread::spawn("memory-file-sync", move || loop {
            let cfg = codeterm_service_core::config::memory_config();
            let sleep_secs = if cfg.file_sync_enabled {
                cfg.file_sync_interval_minutes.max(5) * 60
            } else {
                300
            };
            std::thread::sleep(std::time::Duration::from_secs(sleep_secs));
            if codeterm_service_core::config::memory_config().file_sync_enabled {
                if let Err(e) = mem::file_sync::sync_all(&svc.mem) {
                    log::warn!("[file-sync] {}", e);
                }
            }
        });
    }

    let bundles = {
        let api_config = states.config.lock().api.clone();
        let bundles =
            codeterm_service_core::startup::frontend_bundles_open(&states.event_broadcaster);
        if let Some(bundles) = &bundles {
            codeterm_service_core::startup::frontend_bundles_check(bundles.clone());
        }
        assert!(service_state.set_ext(states.rest_api.clone()).is_ok());
        let rest_inputs = codeterm_api::routes::AppState {
            frontend_dev_url,
            svc: service_state.clone(),
            cron: cron_state.clone(),
            cron_wake: cron_wake.clone(),
            api_port: api_config.port,
            update_state: states.update.clone(),
            config: states.config.clone(),
            mesh: states.mesh.clone(),
            keepawake: states.keepawake.clone(),
            resize_owner: states.resize_owner.clone(),
            rest_api: states.rest_api.clone(),
            test_clock: test_clock.clone(),
            events: states.event_broadcaster.clone(),
            web_dist: resolve_web_dist(resources),
        };
        let router =
            codeterm_api::routes::create_router_with_frontend(rest_inputs, bundles.clone());
        let router = match quit_port {
            Some(port) => codeterm_api::app_quit_routes::with_port(router, port),
            None => router,
        };
        assert!(service_state
            .set_ext(codeterm_service_core::ports::rest::LocalApiRouter(
                router.clone(),
            ))
            .is_ok());
        codeterm_socket::server::SocketServer::start(
            states.mux.clone(),
            states.pty.clone(),
            states.notif.clone(),
            states.output_buffer.clone(),
            service_state.clone(),
        )?;
        let remote_bind = codeterm_service_core::startup::rest_api_additional_bind(
            &api_config,
            cfg!(debug_assertions),
        );
        let listener_allowed =
            match codeterm_service_core::config::validate_api_trust_boundary(&api_config)
                .and_then(|()| remote_bind.as_ref().map(|_| ()).map_err(Clone::clone))
            {
                Ok(()) => true,
                Err(error) => {
                    log::error!("[api] listener disabled by trust-boundary policy: {error}");
                    false
                }
            };
        if api_config.enabled && listener_allowed {
            let weak_mesh = Arc::downgrade(&states.mesh);
            let mesh_config = states.config.clone();
            states.rest_api.listeners.configure(
                remote_bind.expect("validated remote bind"),
                Arc::new(move |ready| {
                    if let Some(mesh) = weak_mesh.upgrade() {
                        let changed = {
                            let mut state = mesh.lock();
                            state.set_ready_listeners(ready)
                        };
                        if changed && mesh_config.lock().mesh.enabled {
                            codeterm_service_core::async_runtime::spawn(async move {
                                let report =
                                    codeterm_service_core::mesh::broadcast_announce(&mesh, None)
                                        .await;
                                if !report.errors.is_empty() {
                                    log::warn!(
                                        "[mesh] listener-set announcement failed: {:?}",
                                        report.errors
                                    );
                                }
                            });
                        }
                    }
                }),
            );
            states
                .rest_api
                .listeners
                .set_proof_token(api_config.token.clone());
            states
                .rest_api
                .listeners
                .publish(states.rest_api.listeners.ready());
        }
        bundles
    };

    let (general_ready, general_initialized) = std::sync::mpsc::channel();
    let (restore_ready, restore_initialized) = std::sync::mpsc::channel();
    // Start General Agent PTY + launch agent (workspace/tab already created in init_states)
    if states.storage.storage().is_some() {
        let config = states.config.lock().general_agent.clone();
        if config.enabled && config.first_run_complete {
            let svc = service_state.clone();
            let mux = states.mux.clone();
            let pty = states.pty.clone();
            let screen_state = states.screen_state.clone();
            let config_state = states.config.clone();
            codeterm_util::named_thread::spawn("general-agent-startup", move || {
                if !service_initialized(general_initialized) {
                    log::error!(
                        "[general-agent] service initialization acknowledgement unavailable"
                    );
                    return;
                }
                log::info!("[general-agent] Startup beginning...");
                service::general_agent::start_general_agent(
                    &svc,
                    &mux,
                    &pty,
                    &screen_state,
                    &config_state,
                );
            });
        }
    }
    if states.storage.storage().is_some() {
        service::turn_resume::arm(&service_state);
        start_restored_pty_backends(service_state.clone(), restore_initialized);
    }

    {
        let mesh_config = states.config.lock().mesh.clone();
        if mesh_config.enabled {
            let mesh = states.mesh.clone();
            mesh::discovery::detect_tailscale(&mesh);
            mesh::discovery::detect_local_ip(&mesh);
        }
    }
    service_state.start_account_mesh_transport();

    let integrations = states.config.lock().integrations.clone();
    if integrations.telegram_enabled {
        codeterm_service_core::startup::spawn_telegram_bridge(service_state.clone());
    }
    let _ = general_ready.send(());
    let _ = restore_ready.send(());
    Ok(bundles)
}

/// Locate the built React bundle for the browser mirror.
/// Dev callers name mutable bundles explicitly; installed builds use only packaged assets.
fn resolve_web_dist(resource_dir: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let is_dev = std::env::var("CODETERM_DEV").as_deref() == Ok("1");
    let explicit = std::env::var("CODETERM_WEB_DIST")
        .ok()
        .map(std::path::PathBuf::from);
    let packaged = resource_dir.into_iter().map(|dir| dir.join("dist"));
    select_web_dist(is_dev, explicit, packaged)
}

fn select_web_dist(
    is_dev: bool,
    explicit: Option<std::path::PathBuf>,
    packaged: impl IntoIterator<Item = std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    for cand in explicit.filter(|_| is_dev).into_iter().chain(packaged) {
        if cand.join("index.html").exists() {
            log::info!("[web] serving browser UI from {}", cand.display());
            return Some(cand);
        }
    }
    log::warn!("[web] browser UI disabled: frontend bundle is missing from the installation");
    None
}

#[cfg(test)]
mod tests;
