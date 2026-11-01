use crate::{
    bootstrap::{
        channel, process,
        protocol::{ServerReady, BOOTSTRAP_PROTOCOL},
    },
    launch::LaunchOptions,
};
use codeterm_paths::instance_identity::ownership_registry;
use codeterm_service_core::{
    ports::host::{NoShell, ProcessPaths, ProcessShell, UiEvents},
    PathOwnership, ResolvedInstancePaths,
};
use codeterm_types::native_update::{NativeUpdateRequest, NativeUpdateResult};
use std::{io, sync::Arc};

type NativeEndpoint =
    crate::native_host::NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>;
type NativeServer = crate::native_host::NativeHostServer<NativeUpdateRequest, NativeUpdateResult>;
type NativeEventPort = Arc<parking_lot::RwLock<Option<NativeEndpoint>>>;

struct ServerEvents {
    events: codeterm_events::EventBroadcasterState,
    native: NativeEventPort,
}
impl UiEvents for ServerEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        self.events.publish_generic(
            event,
            codeterm_events::web_safe_event_payload(event, payload),
        );
    }
    fn emit_native(&self, event: &str, payload: serde_json::Value) {
        let endpoint = self.native.read().clone();
        if let Some(endpoint) = endpoint {
            endpoint.emit_native(event, payload);
        }
    }
    fn emit_to(&self, target: &str, event: &str, payload: serde_json::Value) {
        let endpoint = self.native.read().clone();
        if let Some(endpoint) = endpoint {
            endpoint.emit_to(target, event, payload);
        }
    }
}

fn ownership_registry_for(
    options: &LaunchOptions,
    identity: &ResolvedInstancePaths,
) -> io::Result<std::path::PathBuf> {
    if matches!(
        &options.instance,
        codeterm_types::server_bootstrap::InstanceSelector::IsolatedVerification { .. }
    ) {
        Ok(identity.data().join(".instance-ownership"))
    } else {
        ownership_registry()
    }
}

pub fn run(
    options: LaunchOptions,
    acknowledge: impl FnOnce(&ServerReady, Option<&str>) -> io::Result<()>,
) -> io::Result<()> {
    let boot_started = std::time::Instant::now();
    options.validate(cfg!(debug_assertions))?;
    let development_owner =
        crate::launch_ownership::watch_owner(options.development_owner.as_ref())?;
    let job = if matches!(
        &options.instance,
        codeterm_types::server_bootstrap::InstanceSelector::IsolatedVerification { .. }
    ) {
        None
    } else {
        Some(crate::launch::ensure_detached()?)
    };
    #[cfg(windows)]
    codeterm_service_core::pty::manager::configure_windows_pty_child_launcher(
        std::env::current_exe()?,
    )
    .map_err(io::Error::other)?;
    crate::launch::install_selectors(&options)?;
    let identity = crate::launch::verify_identity(&options, ResolvedInstancePaths::current()?)?;
    let ownership_registry = ownership_registry_for(&options, &identity)?;
    let owner = PathOwnership::try_acquire(identity.clone(), &ownership_registry)?;
    let owner = Box::leak(Box::new(owner));
    let process = process::own()?;
    let provenance = process.provenance()?;
    let resources = options.resources.canonicalize()?;
    let native_inventory = crate::native_inventory::load(&resources)?;
    let shell = Arc::new(ProcessShell {
        paths: ProcessPaths {
            executable: provenance.executable.clone(),
            config: identity
                .config()
                .parent()
                .ok_or_else(|| io::Error::other("Config directory missing"))?
                .into(),
            resources: resources.clone(),
        },
        native: Arc::new(NoShell),
    });
    let (frontend_dev_url, frontend_identifier) = development_frontend_configuration(
        cfg!(debug_assertions),
        matches!(
            &options.instance,
            codeterm_types::server_bootstrap::InstanceSelector::Development { .. }
        ),
        &std::env::var("CODETERM_VITE_PORT").unwrap_or_default(),
        &std::env::var("CODETERM_DEV_IDENTIFIER").unwrap_or_default(),
    )?;
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_build_info::install_version(env!("DOMIOS_VERSION"));
    crate::headless_update::register_if_managed();
    crate::bundle::install();
    codeterm_orchestration::register_orchestration();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    codeterm_service_core::async_runtime::install(runtime.handle().clone());
    let wire_identity = codeterm_types::server_bootstrap::ServerIdentity {
        data: identity.data().into(),
        config: identity.config().into(),
    };
    let listener = runtime.block_on(async { channel::bind(&wire_identity) })?;
    let mut audit = audit_sink(&options, &identity)?;
    note(&mut audit, "mesh")?;
    let _lifetime = if matches!(
        &options.instance,
        codeterm_types::server_bootstrap::InstanceSelector::IsolatedVerification { .. }
    ) {
        None
    } else {
        Some(Box::leak(Box::new(
            codeterm_service_core::acquire_instance_lifetime()?,
        )))
    };
    codeterm_service_core::crash::install();
    crate::startup::setup_logging();
    if job == Some(crate::launch::JobMembership::SurvivesLauncher) {
        log::info!("[bootstrap] server runs inside a job object that survives its launcher");
    }
    note(&mut audit, "crash")?;
    codeterm_service_core::crash::record_startup();
    note(&mut audit, "config-credential-hook-state")?;
    let native_events: NativeEventPort = Arc::new(parking_lot::RwLock::new(None));
    let diagnostics_native = native_events.clone();
    let diagnostics_port =
        codeterm_api::health_routes::NativeDiagnosticsPort(Arc::new(move || {
            diagnostics_native.read().as_ref()?.cdp_port()
        }));
    let frontend_native = native_events.clone();
    let frontend_identity = codeterm_api::health_routes::NativeFrontendIdentity {
        port: Arc::new(move || frontend_native.read().as_ref()?.frontend_port()),
        identifier: frontend_identifier,
    };
    let prepared = crate::startup::early_api::PreparedStartup::new(
        frontend_dev_url.as_ref(),
        diagnostics_port.clone(),
        frontend_identity.clone(),
    )
    .map_err(io::Error::other)?;
    let address = runtime
        .block_on(prepared.listening())
        .map_err(io::Error::other)?;
    log::debug!(
        "[boot-operation] API listener health-proven since_process_entry_ms={}",
        boot_started.elapsed().as_millis()
    );
    let (exact_quit, mut headless_quit) = crate::exact_quit::ExactQuitCoordinator::new();
    let ready = ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: codeterm_service_core::frontend_bundle_api_version::API_VERSION,
        build: codeterm_build_info::sha().into(),
        instance_id: codeterm_service_core::paths::codeterm_instance_id().into(),
        identity: wire_identity.clone(),
        epoch: codeterm_service_core::crash::lifecycle::current_start_epoch()
            .ok_or_else(|| io::Error::other("Server epoch unavailable"))?
            .into(),
        process: provenance,
        api_address: address,
        socket_address: codeterm_service_core::paths::socket_display_path(),
        bootstrap_address: channel::address(&wire_identity)?,
        resources: resources.clone(),
    };
    log::debug!("[bootstrap] native listener binding started");
    let native_server = match native_inventory {
        Some(manifest)
            if matches!(
                manifest.client_identity,
                codeterm_types::server_bootstrap::ClientIdentity::None
            ) =>
        {
            crate::native_host::verify_component(&manifest, &ready, &ready.resources)
                .map(|_| None)
                .map_err(crate::native_host::NativeHostBindError::InventoryRejected)
        }
        Some(manifest) => {
            let origins = codeterm_service_core::startup::frontend_shell_declared_frontend_origins(
                &prepared.config.lock().api,
                frontend_dev_url.as_ref(),
            )
            .into_iter()
            .collect();
            runtime
                .block_on(async { NativeServer::bind(ready.clone(), manifest, origins) })
                .map(Some)
        }
        None => Ok(None),
    };
    let native_server = match optional_native_listener(native_server) {
        Ok(server) => server,
        Err(error) => {
            prepared.api.fail("integrity");
            let _ = runtime.block_on(prepared.control.shutdown_and_wait());
            runtime.shutdown_timeout(std::time::Duration::from_secs(6));
            return Err(error);
        }
    };
    log::debug!("[bootstrap] native listener binding completed");
    let attached_once = native_server.as_ref().map(NativeServer::attached_once);
    let worker_cli =
        match crate::resources::pin_worker_cli(shell.as_ref(), env!("SERVER_CLI_SHA256")) {
            Ok(worker_cli) => worker_cli,
            Err(error) => {
                prepared.api.fail("integrity");
                let _ = runtime.block_on(prepared.control.shutdown_and_wait());
                runtime.shutdown_timeout(std::time::Duration::from_secs(6));
                return Err(error);
            }
        };
    prepared.api.complete("integrity");
    if let Some(server) = &native_server {
        *native_events.write() = Some(server.endpoint());
    }
    exact_quit.install(
        ready.clone(),
        native_server.as_ref().map(|server| server.endpoint()),
    );
    let token = prepared.config.lock().api.token.clone();
    log::debug!("[bootstrap] inherited ready acknowledgement started");
    let acknowledgement = acknowledge(&ready, token.as_deref());
    log::debug!(
        "[bootstrap] inherited ready acknowledgement completed: {}",
        acknowledgement.is_ok()
    );
    if job.is_some() {
        if let Err(error) = crate::launch::release_launch_stderr() {
            log::warn!("[bootstrap] launcher stderr release failed: {error}");
        }
    }
    let startup_api = prepared.api.clone();
    let startup_control = prepared.control.clone();
    let service_native = native_events.clone();
    let service_quit = exact_quit.clone();
    let probe_workers = audit.is_some();
    let worker_data = identity.data().to_path_buf();
    let mut initializing = runtime.spawn_blocking(move || {
        codeterm_util::sensitive_state::tighten_existing(&worker_data);
        if let Ok(config) = codeterm_service_core::paths::config_path() {
            if config.parent() != Some(worker_data.as_path()) {
                codeterm_util::sensitive_state::tighten_config_recovery(&config);
            }
        }
        let result = (|| {
            let worker_observer = if probe_workers {
                Some(
                    crate::worker_observer::start(&worker_data, worker_cli.path.clone())
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            };
            let running = crate::composition::compose_observed(
                |events| {
                    Arc::new(ServerEvents {
                        events,
                        native: service_native.clone(),
                    })
                },
                shell,
                frontend_dev_url,
                Some(resources),
                worker_observer,
                Some(codeterm_api::app_quit_routes::AppQuitPortHandle(
                    service_quit,
                )),
                prepared,
            )?;
            running
                .service
                .register_host_extension(Arc::new(diagnostics_port))
                .map_err(|_| "native diagnostics port already registered".to_string())?;
            running
                .service
                .register_host_extension(Arc::new(frontend_identity))
                .map_err(|_| "native frontend identity already registered".to_string())?;
            running
                .service
                .register_host_extension(Arc::new(
                    codeterm_commands::application_host::ApplicationHostState(Arc::new(
                        crate::native_application_host::NativeApplicationHost {
                            native: service_native,
                            service: running.service.clone(),
                            update_state: running.states.update.clone(),
                        },
                    )),
                ))
                .map_err(|_| "native application host already registered".to_string())?;
            startup_api.activate(
                running
                    .service
                    .ext::<codeterm_service_core::ports::rest::LocalApiRouter>()
                    .0
                    .clone(),
            )?;
            Ok::<_, String>((running, worker_cli))
        })();
        if result.is_err() {
            startup_api.fail("initialization");
        }
        result
    });
    let mut running = None;
    let control_result = continue_after_ack(acknowledgement, || {
        runtime.block_on(async {
            let control = channel::serve(listener, ready, token);
            tokio::pin!(control);
            let native = serve_native(native_server, native_events);
            tokio::pin!(native);
            let orphaned = async {
                if let Some(owner) = development_owner {
                    owner.await.map_err(io::Error::other)??;
                    let attached = attached_once.as_ref().is_some_and(|value| {
                        value.load(std::sync::atomic::Ordering::Acquire)
                    });
                    if crate::launch_ownership::retire_unattached(true, attached) {
                        return Ok::<(), io::Error>(());
                    }
                }
                std::future::pending::<io::Result<()>>().await
            };
            tokio::pin!(orphaned);
            loop { tokio::select! {
                result = &mut initializing, if running.is_none() => {
                    running = Some(result.map_err(io::Error::other).and_then(|result| result.map_err(io::Error::other)));
                    if running.as_ref().is_some_and(Result::is_err) {
                        return Err(io::Error::other("Service initialization failed"));
                    }
                }
                result = &mut control => return result.map(|completion| Some(QuitControl::Bootstrap(completion))),
                _ = async {
                    while let Some(expected) = headless_quit.recv().await {
                        if exact_quit.admit_headless(&expected) { return; }
                    }
                    std::future::pending::<()>().await;
                } => return Ok(Some(QuitControl::Headless)),
                result = shutdown_signal() => return result.map(|_| None),
                result = &mut orphaned => return result.map(|_| None),
                result = &mut native => return result.map(|_| None),
            } }
        })
    });
    let (running, _worker_cli) = match running.unwrap_or_else(|| {
        runtime
            .block_on(initializing)
            .map_err(io::Error::other)
            .and_then(|result| result.map_err(io::Error::other))
    }) {
        Ok(running) => running,
        Err(error) => {
            let _ = runtime.block_on(startup_control.shutdown_and_wait());
            runtime.shutdown_timeout(std::time::Duration::from_secs(6));
            return Err(error);
        }
    };
    let result = finish_server_control(
        control_result,
        || shutdown(&running),
        |completion, cleanup| {
            let result = match completion {
                QuitControl::Bootstrap(completion) => {
                    runtime.block_on(completion.complete(cleanup))
                }
                QuitControl::Headless => cleanup,
            };
            exact_quit.complete(&result);
            result
        },
    );
    if exact_quit.completion_sent() {
        if let Err(error) = runtime.block_on(running.states.rest_api.shutdown_and_wait()) {
            log::debug!("[exact-quit] HTTP response drain failed: {error}");
        }
    }
    runtime.shutdown_timeout(std::time::Duration::from_secs(6));
    drop(process);
    let _ = owner;
    result
}

enum QuitControl {
    Bootstrap(channel::ShutdownCompletion),
    Headless,
}

fn finish_server_control<T>(
    control: io::Result<Option<T>>,
    cleanup: impl FnOnce() -> io::Result<()>,
    complete: impl FnOnce(T, io::Result<()>) -> io::Result<()>,
) -> io::Result<()> {
    let cleanup = cleanup();
    match control {
        Ok(Some(completion)) => complete(completion, cleanup),
        Ok(None) => cleanup,
        Err(error) => cleanup.and(Err(error)),
    }
}

fn development_frontend_configuration(
    debug: bool,
    development: bool,
    port: &str,
    identifier: &str,
) -> io::Result<(Option<url::Url>, String)> {
    match codeterm_frontend_shell::development_runtime_frontend(
        debug,
        development,
        port,
        identifier,
    )
    .map_err(io::Error::other)?
    {
        Some(frontend) => Ok((Some(frontend.origin), frontend.identifier)),
        None => Ok((None, String::new())),
    }
}

#[cfg(test)]
fn development_frontend_origin(
    debug: bool,
    development: bool,
    origin: &str,
) -> io::Result<Option<url::Url>> {
    if !debug || !development || origin.is_empty() {
        return Ok(None);
    }
    let url = url::Url::parse(origin).map_err(io::Error::other)?;
    codeterm_frontend_shell::development_frontend(true, Some(&url)).map_err(io::Error::other)
}

fn optional_native_listener<T>(
    result: Result<Option<T>, crate::native_host::NativeHostBindError>,
) -> io::Result<Option<T>> {
    match result {
        Ok(server) => Ok(server),
        Err(crate::native_host::NativeHostBindError::InventoryRejected(error)) => Err(error),
        Err(crate::native_host::NativeHostBindError::ListenerUnavailable(error)) => {
            log::debug!("[native-host] listener unavailable; headless service retained: {error}");
            Ok(None)
        }
    }
}

fn continue_after_ack<T>(
    acknowledgement: io::Result<()>,
    serve: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    if let Err(error) = acknowledgement {
        if !matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::NotConnected
        ) {
            return Err(error);
        }
        log::debug!("[bootstrap] launch observer disconnected after service readiness");
    }
    serve()
}

async fn serve_native(server: Option<NativeServer>, events: NativeEventPort) -> io::Result<()> {
    if let Some(server) = server {
        if let Err(error) = server.serve().await {
            log::debug!("[native-host] listener unavailable; headless service retained: {error}");
        }
        *events.write() = None;
    }
    std::future::pending().await
}

async fn shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result, _ = term.recv() => Ok(()) }
    }
    #[cfg(windows)]
    {
        tokio::signal::ctrl_c().await
    }
}

fn shutdown(running: &crate::ComposedService) -> io::Result<()> {
    use codeterm_service_core::{crash, service::session_persist, startup};
    crash::shutdown::mark_shutting_down_for(crash::shutdown::ShutdownReason::Signal);
    let spawn_gate = running.states.pty.lock().spawn_gate();
    spawn_gate.quiesce();
    let surfaces =
        codeterm_service_core::service::surface::begin_surface_shutdown(&running.service);
    let mut errors = Vec::new();
    if let Err(error) =
        spawn_gate.wait_for_idle(std::time::Instant::now() + std::time::Duration::from_secs(6))
    {
        errors.push(error);
    }
    match session_persist::write_session_snapshot(&running.service, &running.service.mux) {
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved) => {}
        Ok(outcome) => errors.push(format!("Shutdown snapshot refused: {outcome:?}")),
        Err(error) => errors.push(error),
    }
    if let Err(error) = session_persist::stop_worker(&running.service) {
        errors.push(error);
    }
    if let Err(error) =
        codeterm_service_core::service::agent_runtime::engine::shutdown_blocking(&running.service)
    {
        errors.push(format!("Agent runtime shutdown failed: {error}"));
    }
    #[cfg(windows)]
    let owned_tree = codeterm_util::process_tree::OwnedProcessTree::capture_current_server()
        .map_err(|error| io::Error::other(format!("Owned process tree capture failed: {error}")));
    let reapers = running.states.pty.lock().begin_shutdown();
    #[cfg(windows)]
    if let Err(error) = owned_tree.and_then(|tree| {
        tree.terminate_remaining(std::time::Duration::from_secs(6))
            .map_err(|error| io::Error::other(format!("Owned process tree exit failed: {error}")))
    }) {
        errors.push(error.to_string());
    }
    match reapers {
        Ok(reapers) => {
            for reaper in reapers {
                if let Err(error) = reaper.wait_bounded(std::time::Duration::from_secs(6)) {
                    errors.push(error.to_string());
                }
            }
        }
        Err(error) => errors.push(error.to_string()),
    }
    surfaces.wait();
    let stopped_git = codeterm_plugins::stop_live_git_children();
    if stopped_git > 0 {
        log::info!(
            "[shutdown] stopped {stopped_git} plugin git child(ren) still writing the data dir"
        );
    }
    if let Err(error) = startup::shutdown_bounded(&running.states.storage) {
        errors.push(error.to_string());
    }
    if let Err(error) = running.service.mem.checkpoint_and_close() {
        errors.push(error.to_string());
    }
    codeterm_service_core::keepawake::cleanup_active_children_for_exit_with_state(
        &running.states.keepawake,
    );
    crash::exit::shutdown_state_persistence_result(&errors).map_err(io::Error::other)
}

fn audit_sink(
    options: &LaunchOptions,
    identity: &ResolvedInstancePaths,
) -> io::Result<Option<std::fs::File>> {
    let Some(_) = &options.probe_sink else {
        return Ok(None);
    };
    std::fs::create_dir_all(identity.data())?;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(identity.data().join("process-init.jsonl"))
        .map(Some)
}

fn note(sink: &mut Option<std::fs::File>, phase: &str) -> io::Result<()> {
    use std::io::Write;
    if let Some(file) = sink {
        writeln!(
            file,
            "{}",
            serde_json::json!({ "pid": std::process::id(), "phase": phase })
        )?;
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
