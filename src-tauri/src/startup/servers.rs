//! Start socket server, REST API server, cron scheduler, GA, mesh, bridge.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use crate::multiplexer::types::TabBacking;
use crate::service::ServiceState;
use crate::{api, cron, mem, mesh, service, socket, CronState, CronWake};

use super::states::AppStates;

fn should_materialize_restored_backend(
    restored_from_snapshot: bool,
    protected: bool,
    backing: &TabBacking,
    pty_id: Option<&str>,
) -> bool {
    restored_from_snapshot && !protected && matches!(backing, TabBacking::Pty) && pty_id.is_none()
}

pub(crate) const FRONTEND_READINESS_DEADLINE_ELAPSED: &str = "frontend_readiness_deadline_elapsed";

fn start_restored_pty_backends(service_state: ServiceState, frontend_readiness_generation: u64) {
    codeterm_util::named_thread::spawn("restore-pty-backends", move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        materialize_restored_backends(
            || super::wait_for_frontend_ready_until(frontend_readiness_generation, deadline),
            || note_frontend_deadline(&service_state),
            || restored_backend_tab_ids(&service_state),
            |tab_id| service_state.ensure_tab_pty_for_send(tab_id),
        );
    });
}

/// A missed frontend deadline never abandons restored agents: the backend claims
/// their PTYs, and a later frontend mount reuses them through the same tab claim.
fn materialize_restored_backends<W, D, T, M>(
    wait_frontend: W,
    on_deadline: D,
    tab_ids: T,
    mut materialize: M,
) -> Vec<String>
where
    W: FnOnce() -> bool,
    D: FnOnce(),
    T: FnOnce() -> Vec<String>,
    M: FnMut(&str) -> Result<(), String>,
{
    if !wait_frontend() {
        log::warn!(
            "[startup-pty] frontend readiness deadline elapsed; materializing restored backends without the frontend"
        );
        on_deadline();
    }
    let tab_ids = tab_ids();
    for tab_id in &tab_ids {
        if let Err(error) = materialize(tab_id) {
            log::error!("[startup-pty] tab={tab_id} materialization failed: {error}");
        }
    }
    tab_ids
}

fn restored_backend_tab_ids(service_state: &ServiceState) -> Vec<String> {
    service_state
        .mux
        .read()
        .snapshot()
        .tabs
        .into_values()
        .filter(|tab| {
            should_materialize_restored_backend(
                tab.restored_from_snapshot,
                tab.protected,
                &tab.backing,
                tab.pty_id.as_deref(),
            )
        })
        .map(|tab| tab.id)
        .collect()
}

fn note_frontend_deadline(service_state: &ServiceState) {
    let restored_ids: std::collections::BTreeSet<String> = service_state
        .mux
        .read()
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role.is_some() || tab.agent_type.is_some())
        .map(|tab| tab.id.clone())
        .collect();
    crate::crash::restore_report::note_backend_materialization(
        FRONTEND_READINESS_DEADLINE_ELAPSED,
        &restored_ids,
    );
}

const GENERAL_AGENT_FRONTEND_DEADLINE_REASON: &str =
    "frontend did not report readiness before the launch deadline";

fn report_general_agent_frontend_deadline(service: &crate::service::ServiceContext) {
    log::error!("[general-agent] {GENERAL_AGENT_FRONTEND_DEADLINE_REASON}");
    // emit_boot_stage re-locks mux; a scrutinee guard would live through the call.
    let general_tab_id = service
        .mux
        .read()
        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS);
    if let Some(tab_id) = general_tab_id {
        service.emit_boot_stage(
            &tab_id,
            codeterm_types::pty::BootStage::SpawnFailed,
            Some(GENERAL_AGENT_FRONTEND_DEADLINE_REASON),
        );
    }
}

/// Start socket server, REST API server, and cron scheduler.
pub(crate) fn start_servers(app: &tauri::App, states: &AppStates, service_state: &ServiceState) {
    let app_handle = app.handle().clone();
    super::state_incidents::install_notifier(service_state);
    if let Some(reason) = states.storage.unavailable_reason() {
        super::state_incidents::notify_storage_unavailable(
            service_state,
            &crate::paths::data_dir().join("state.db"),
            reason,
        );
        if let Err(error) = crate::service::session_persist::set_storage_unavailable(
            &app_handle,
            reason.to_string(),
        ) {
            log::error!("[session-persist] unavailable backend setup failed: {error}");
        }
    } else if let Some(storage) = states.storage.storage() {
        let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
        if let Err(error) =
            crate::service::session_persist::install_storage_desk_adapter(&app_handle, adapter)
        {
            log::error!("[session-persist] Storage backend setup failed: {error}");
        }
    }

    if states.boot_resync && states.storage.storage().is_some() {
        let resync = crate::service::session_persist::write_session_snapshot_with_intent(
            &app_handle,
            &states.mux,
            crate::session::store::SaveIntent::AuthorizedReduction,
        );
        if let Err(error) = resync {
            log::error!("[restore] Storage desk boot resync failed: {error}");
        }
    }
    let frontend_readiness_generation = super::arm_frontend_readiness();

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
    let logs_root = crate::paths::data_dir().join("crons");
    let cron_store = match states.storage.storage() {
        Some(storage) => cron::store::CronStore::open_storage(
            storage,
            logs_root.clone(),
            cron_clock.clone(),
            cron::store::CronStorageOptions::default(),
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
        let mut cs = cron_state.lock();
        let mem_cfg = crate::config::memory_config();
        let default_agent = crate::config::default_agent_provider();
        match mem::dreaming::ensure_dreaming_cron(
            &mut crate::mem_host::CronSchedulerPort(&mut cs),
            &mem_cfg,
            &default_agent,
            &crate::service::spawn::CatalogBackgroundModelPort,
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
        cron::recovery::RestoredCronSession::from_mux(&mux)
    };
    let scheduler_ready = port_ready
        && match cron::recovery::reconcile_restored_runs(&cron_state, &restored_cron_session) {
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

    app.manage(cron_state.clone());
    app.manage(cron_wake.clone());
    let fence_wake = cron_wake.clone();
    cron_state
        .lock()
        .set_fence_notifier(move || fence_wake.notify_all());
    if scheduler_ready {
        cron::scheduler::start(cron_state.clone(), cron_wake.clone(), service_state.clone());
    }
    let sentinel_clock: cron::clock::SharedClock = test_clock
        .clone()
        .map(|clock| clock as cron::clock::SharedClock)
        .unwrap_or_else(cron::clock::system_clock);
    if let Some(storage) = states.storage.storage() {
        crate::sentinel::scheduler::start(service_state.clone(), sentinel_clock, storage);
    }

    tauri::async_runtime::block_on(async {
        let init_started = std::time::Instant::now();
        if let Err(error) = service_state.secrets.read().await.init().await {
            log::warn!("[secrets] init failed: {error}");
        } else {
            log::info!(
                "[secrets] init done in {}ms",
                init_started.elapsed().as_millis()
            );
        }
    });

    {
        let svc = service_state.clone();
        codeterm_util::named_thread::spawn("memory-file-sync", move || loop {
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
            rest_api: states.rest_api.clone(),
            test_clock: test_clock.clone(),
            events: states.event_broadcaster.clone(),
            web_dist: resolve_web_dist(app.path().resource_dir().ok()),
            snapshot_bound: api::snapshot_bound::SnapshotBound::new(),
        };
        let bundles = super::frontend_bundles::open(&states.event_broadcaster);
        if let Some(bundles) = &bundles {
            app.manage(bundles.clone());
            super::frontend_bundles::check(bundles.clone());
        }
        let router = api::routes::create_router_with_frontend(api_state, bundles);
        app.manage(api::routes::LocalApiRouter(router.clone()));
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
        let remote_bind =
            super::rest_api::listeners::additional_bind(&api_config, cfg!(debug_assertions));
        let listener_allowed = match crate::config::validate_api_trust_boundary(&api_config)
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
                            let changed = state.ready_listeners != ready;
                            state.ready_listeners = ready;
                            changed
                        };
                        if changed {
                            crate::mesh::discovery::stop_broadcast(&mesh);
                            if mesh_config.lock().mesh.enabled {
                                tauri::async_runtime::spawn(async move {
                                    let report = crate::mesh::broadcast_announce(&mesh, None).await;
                                    if !report.errors.is_empty() {
                                        log::warn!(
                                            "[mesh] listener-set announcement failed: {:?}",
                                            report.errors
                                        );
                                    }
                                });
                            }
                        }
                    }
                }),
            );
            let bind_addr = crate::config::canonical_api_bind_addr(&api_config)
                .expect("validated API host must have a canonical socket address");
            states
                .rest_api
                .set_runtime_policy(super::RestApiRuntimePolicy {
                    enabled: true,
                    cors: api_config.cors,
                    cors_allow_origins: if api_config.cors {
                        api_config.cors_allow_origins.clone()
                    } else {
                        Vec::new()
                    },
                });
            let port = api_config.port;
            if !bind_addr.ip().is_loopback() {
                log::warn!(
                    "[api] explicit non-loopback listener {bind_addr} exposes authenticated control to the network"
                );
            }
            std::env::set_var("CODETERM_API_PORT", port.to_string());
            let control = states.rest_api.clone();
            super::rest_api::spawn(control, bind_addr.to_string(), router);
        }
    }

    // Start General Agent PTY + launch agent (workspace/tab already created in init_states)
    if states.storage.storage().is_some() {
        let config = states.config.lock().general_agent.clone();
        if config.enabled && config.first_run_complete {
            let svc = service_state.clone();
            let mux = states.mux.clone();
            let pty = states.pty.clone();
            let screen_state = states.screen_state.clone();
            let config_state = states.config.clone();
            let app_h = app_handle.clone();
            let frontend_ready_deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(30);

            codeterm_util::named_thread::spawn("general-agent-startup", move || {
                if !super::wait_for_frontend_ready_until(
                    frontend_readiness_generation,
                    frontend_ready_deadline,
                ) {
                    report_general_agent_frontend_deadline(&svc);
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
        }
    }
    if states.storage.storage().is_some() {
        service::turn_resume::arm(&service_state);
        start_restored_pty_backends(service_state.clone(), frontend_readiness_generation);
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
    log::warn!("[web] no dist/ bundle found — browser UI disabled (build with `pnpm build`)");
    None
}

#[cfg(test)]
mod tests {
    use super::{
        materialize_restored_backends, report_general_agent_frontend_deadline,
        restored_backend_tab_ids, select_web_dist, should_materialize_restored_backend,
    };
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TabBacking;
    use crate::service::test_fixture::ServiceFixture;
    use codeterm_types::pty::BootStage;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn general_agent_frontend_deadline_releases_mux_for_restored_backends() {
        let (mut mux, general_tab_id) = MultiplexerState::new();
        mux.register_alias(
            crate::service::general_agent::GENERAL_ALIAS,
            &general_tab_id,
        );
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let restored_tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.get_tab_mut(&restored_tab_id)
            .unwrap()
            .restored_from_snapshot = true;
        let fixture = ServiceFixture::new(mux);
        let service = Arc::new(fixture.service.clone());

        let (general_tx, general_rx) = mpsc::channel();
        let general_service = service.clone();
        std::thread::spawn(move || {
            report_general_agent_frontend_deadline(&general_service);
            general_tx.send(()).unwrap();
        });
        let (restore_tx, restore_rx) = mpsc::channel();
        let restore_service = service.clone();
        std::thread::spawn(move || {
            let attempted = materialize_restored_backends(
                || false,
                || {},
                || restored_backend_tab_ids(&restore_service),
                |_| Ok(()),
            );
            restore_tx.send(attempted).unwrap();
        });

        general_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the General Agent deadline path must not self-deadlock on mux");
        let attempted = restore_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("restored backends must materialize after a missed frontend deadline");

        assert_eq!(attempted, vec![restored_tab_id]);
        let general = service
            .mux
            .read()
            .get_tab(&general_tab_id)
            .cloned()
            .unwrap();
        assert_eq!(general.boot_stage, Some(BootStage::SpawnFailed));
    }

    #[test]
    fn restore_contract_missed_frontend_deadline_still_materializes_restored_agent_panes() {
        let mut deadline_noted = false;
        let mut materialized = Vec::new();
        let attempted = materialize_restored_backends(
            || false,
            || deadline_noted = true,
            || {
                ["orchestrator", "codex-worker", "planner"]
                    .map(String::from)
                    .to_vec()
            },
            |tab_id| {
                materialized.push(tab_id.to_string());
                if tab_id == "codex-worker" {
                    Err("provider launch refused".to_string())
                } else {
                    Ok(())
                }
            },
        );

        assert!(
            deadline_noted,
            "a missed deadline is surfaced, never silent"
        );
        assert_eq!(
            materialized,
            vec!["orchestrator", "codex-worker", "planner"]
        );
        assert_eq!(
            attempted, materialized,
            "one failed pane never stops the rest"
        );
    }

    #[test]
    fn restore_contract_ready_frontend_materializes_without_a_deadline_note() {
        let mut deadline_noted = false;
        let mut materialized = Vec::new();
        materialize_restored_backends(
            || true,
            || deadline_noted = true,
            || vec!["worker".to_string()],
            |tab_id| {
                materialized.push(tab_id.to_string());
                Ok(())
            },
        );

        assert!(!deadline_noted);
        assert_eq!(materialized, vec!["worker"]);
    }

    fn web_bundle(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
        let dist = parent.join(name);
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), name).unwrap();
        dist
    }

    #[test]
    fn packaged_web_bundle_is_selected_without_an_explicit_override() {
        let root = tempfile::tempdir().unwrap();
        let packaged = web_bundle(root.path(), "packaged");
        let source_tree = web_bundle(root.path(), "mutable-source-tree");
        let selected = select_web_dist(false, None, [packaged.clone()]);

        assert_eq!(selected, Some(packaged));
        assert_ne!(selected, Some(source_tree));
    }

    #[test]
    fn explicit_dev_bundle_overrides_packaged_assets() {
        let root = tempfile::tempdir().unwrap();
        let packaged = web_bundle(root.path(), "packaged");
        let dev = web_bundle(root.path(), "dev");

        assert_eq!(
            select_web_dist(true, Some(dev.clone()), [packaged]),
            Some(dev)
        );
    }

    #[test]
    fn production_ignores_an_inherited_mutable_override() {
        let root = tempfile::tempdir().unwrap();
        let packaged = web_bundle(root.path(), "packaged");
        let inherited = web_bundle(root.path(), "inherited-dev");

        assert_eq!(
            select_web_dist(false, Some(inherited), [packaged.clone()]),
            Some(packaged),
        );
    }

    #[test]
    fn startup_materializes_only_unprotected_restored_pty_backends() {
        assert!(should_materialize_restored_backend(
            true,
            false,
            &TabBacking::Pty,
            None,
        ));
        assert!(!should_materialize_restored_backend(
            false,
            false,
            &TabBacking::Pty,
            None,
        ));
        assert!(!should_materialize_restored_backend(
            true,
            true,
            &TabBacking::Pty,
            None,
        ));
        assert!(!should_materialize_restored_backend(
            true,
            false,
            &TabBacking::ChatBackend {
                plugin_id: "test".into(),
                session_id: "session".into(),
            },
            None,
        ));
        assert!(!should_materialize_restored_backend(
            true,
            false,
            &TabBacking::Pty,
            Some("live"),
        ));
    }
}
