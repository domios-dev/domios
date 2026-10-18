use crate::{
    bootstrap::{
        channel, process,
        protocol::{ServerReady, BOOTSTRAP_PROTOCOL, FAILURE_DEADLINE},
    },
    launch::LaunchOptions,
};
use codeterm_service_core::{
    ports::host::{NoShell, ProcessPaths, ProcessShell, UiEvents},
    startup::RestApiPhase,
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

pub fn ownership_registry() -> io::Result<std::path::PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| io::Error::other("Home directory unavailable"))?;
    let root = home.join(".codeterm-instance-lifetimes");
    std::fs::create_dir_all(&root)?;
    Ok(root.join("path-owners"))
}

pub fn run(
    options: LaunchOptions,
    acknowledge: impl FnOnce(&ServerReady, Option<&str>) -> io::Result<()>,
) -> io::Result<()> {
    crate::launch::ensure_detached()?;
    crate::launch::install_selectors(&options)?;
    let identity = crate::launch::verify_identity(&options, ResolvedInstancePaths::current()?)?;
    let owner = Box::leak(Box::new(PathOwnership::try_acquire(
        identity.clone(),
        &ownership_registry()?,
    )?));
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
    let worker_cli = crate::resources::pin_worker_cli(shell.as_ref(), env!("SERVER_CLI_SHA256"))?;
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_build_info::install_version(env!("DOMIOS_VERSION"));
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
    let worker_observer = if audit.is_some() {
        Some(crate::worker_observer::start(
            identity.data(),
            worker_cli.path.clone(),
        )?)
    } else {
        None
    };
    note(&mut audit, "mesh")?;
    let _lifetime = Box::leak(Box::new(codeterm_service_core::acquire_instance_lifetime()?));
    codeterm_service_core::crash::install();
    crate::startup::setup_logging();
    note(&mut audit, "crash")?;
    codeterm_service_core::crash::record_startup();
    note(&mut audit, "config-credential-hook-state")?;
    let native_events: NativeEventPort = Arc::new(parking_lot::RwLock::new(None));
    let running = crate::composition::compose_observed(
        |events| {
            Arc::new(ServerEvents {
                events,
                native: native_events.clone(),
            })
        },
        shell,
        None,
        Some(resources.clone()),
        worker_observer,
    )
    .map_err(io::Error::other)?;
    let ready = runtime.block_on(async {
        let mut statuses = running.states.rest_api.subscribe_status();
        tokio::time::timeout(FAILURE_DEADLINE, async {
            loop {
                match statuses.borrow_and_update().phase {
                    RestApiPhase::Listening => break,
                    RestApiPhase::Failed => {
                        return Err(io::Error::other("Server listener initialization failed"))
                    }
                    RestApiPhase::Starting => {}
                }
                statuses.changed().await.map_err(io::Error::other)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Server readiness deadline"))??;
        let address = running
            .states
            .rest_api
            .bound_addr()
            .ok_or_else(|| io::Error::other("Listener ownership unavailable"))?;
        Ok::<_, io::Error>(ServerReady {
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
            resources,
        })
    })?;
    let native_server = native_inventory
        .map(|manifest| {
            let origins = codeterm_service_core::startup::frontend_shell_declared_frontend_origins(
                &running.states.config.lock().api,
                None,
            )
            .into_iter()
            .collect();
            runtime.block_on(async { NativeServer::bind(ready.clone(), manifest, origins) })
        })
        .transpose();
    let native_server = match optional_native_listener(native_server) {
        Ok(server) => server,
        Err(error) => {
            if let Err(cleanup) = shutdown(&running) {
                log::error!("[native-host] refused startup cleanup failed: {cleanup}");
            }
            runtime.shutdown_timeout(std::time::Duration::from_secs(6));
            return Err(error);
        }
    };
    if let Some(server) = &native_server {
        *native_events.write() = Some(server.endpoint());
    }
    let token = running.states.config.lock().api.token.clone();
    let control_result = continue_after_ack(acknowledge(&ready, token.as_deref()), || {
        runtime.block_on(async {
            let control = channel::serve(listener, ready, token);
            tokio::select! {
                result = control => result.map(Some),
                result = shutdown_signal() => result.map(|_| None),
                result = serve_native(native_server, native_events) => result.map(|_| None),
            }
        })
    });
    let result = finish_server_control(
        control_result,
        || shutdown(&running),
        |completion, cleanup| runtime.block_on(completion.complete(cleanup)),
    );
    runtime.shutdown_timeout(std::time::Duration::from_secs(6));
    drop(process);
    let _ = owner;
    result
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
    let sidecars = crash::exit::begin_sidecar_shutdown();
    let surfaces =
        codeterm_service_core::service::surface::begin_surface_shutdown(&running.service);
    let mut errors = Vec::new();
    match session_persist::write_session_snapshot(&running.service, &running.service.mux) {
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved) => {}
        Ok(outcome) => errors.push(format!("Shutdown snapshot refused: {outcome:?}")),
        Err(error) => errors.push(error),
    }
    if let Err(error) = session_persist::stop_worker(&running.service) {
        errors.push(error);
    }
    let reapers = running.states.pty.lock().begin_shutdown();
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
    sidecars.wait();
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
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn process_split_contract_native_listener_failure_is_distinct_from_inventory_rejection() {
        use crate::native_host::NativeHostBindError;
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let unavailable = optional_native_listener::<()>(Err(
                NativeHostBindError::ListenerUnavailable(io::Error::from(kind)),
            ));
            assert!(unavailable.unwrap().is_none());
            let rejected = optional_native_listener::<()>(Err(
                NativeHostBindError::InventoryRejected(io::Error::from(kind)),
            ));
            assert!(rejected.is_err());
        }
    }

    #[test]
    fn process_split_contract_readiness_ack_failure_still_runs_cleanup_once() {
        let controls = Cell::new(0);
        let cleanups = Cell::new(0);
        let control = continue_after_ack(Err(io::Error::from(io::ErrorKind::InvalidData)), || {
            controls.set(controls.get() + 1);
            Ok(None::<()>)
        });
        let result = finish_server_control(
            control,
            || {
                cleanups.set(cleanups.get() + 1);
                Ok(())
            },
            |_, _| panic!("No shutdown was admitted"),
        );
        assert!(result.is_err());
        assert_eq!(controls.get(), 0);
        assert_eq!(cleanups.get(), 1);
    }

    #[test]
    fn process_split_contract_launcher_disconnect_keeps_service_control_independent() {
        let controls = Cell::new(0);
        let result = continue_after_ack(Err(io::Error::from(io::ErrorKind::BrokenPipe)), || {
            controls.set(controls.get() + 1);
            Ok(42)
        })
        .unwrap();
        assert_eq!(result, 42);
        assert_eq!(controls.get(), 1);
    }
}
