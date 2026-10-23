use crate::startup;
use codeterm_service_core::ports::host::{Shell, UiEvents};
use codeterm_service_core::service::ServiceState;
use codeterm_service_core::{agent, bridge, keepawake, mem, plugin, pty, service};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct ComposedService {
    pub states: codeterm_service_core::startup::AppStates,
    pub service: ServiceState,
    pub frontend_bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
}

pub(crate) fn compose_observed(
    ui_factory: impl FnOnce(codeterm_events::EventBroadcasterState) -> Arc<dyn UiEvents>,
    shell: Arc<dyn Shell>,
    frontend_dev_url: Option<url::Url>,
    resource_dir: Option<std::path::PathBuf>,
    worker_observer: Option<std::sync::mpsc::SyncSender<mem::supervisor::observation::WorkerSpawn>>,
    quit_port: Option<codeterm_api::app_quit_routes::AppQuitPortHandle>,
    prepared: startup::early_api::PreparedStartup,
) -> Result<ComposedService, String> {
    let api = prepared.api;
    if let Some(resources) = &resource_dir {
        startup::setup_conpty_host(resources)?;
    }
    codeterm_service_core::raise_fd_limit();
    pty::shell_integration::ensure_scripts();

    let states = startup::init_states(prepared.config, prepared.control);
    api.complete("states");
    let ui = ui_factory(states.event_broadcaster.clone());
    codeterm_service_core::service::seed_bundled_skills();
    codeterm_service_core::service::mirror_skills_to_providers();

    codeterm_socket::register_socket();
    keepawake::reap_orphaned_children_on_startup();
    keepawake::resume_from_disk(&states.keepawake, &ui);

    agent::model_catalog::ensure_readme_installed();
    match codeterm_service_core::secrets::purge_legacy_master_password_file() {
    codeterm_service_core::secrets::LegacyPurge::Absent => {}
    codeterm_service_core::secrets::LegacyPurge::SecurelyErased => {
        log::info!("[secrets] securely erased the retired master-password drop")
    }
    codeterm_service_core::secrets::LegacyPurge::RemovedWithoutOverwrite => log::warn!(
        "[secrets] removed the retired master-password drop WITHOUT overwriting it; the old secret may still be recoverable from disk — rotate it"
    ),
    codeterm_service_core::secrets::LegacyPurge::RemovalFailed { overwritten } => log::warn!(
        "[secrets] the retired master-password drop is still on disk (overwritten={overwritten}); it may still be recoverable — remove and rotate it"
    ),
    codeterm_service_core::secrets::LegacyPurge::InspectionFailed => log::warn!(
        "[secrets] could not inspect the retired master-password drop; assume it is present and recoverable until checked"
    ),
}
    {
        let mut cfg = states.config.lock();
        if codeterm_service_core::secrets::migrate_native_bitwarden_to_plugin(&mut cfg) {
            let snapshot = cfg.clone();
            drop(cfg);
            let _ = codeterm_service_core::config::save_config_pub(&snapshot);
        }
    }

    let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
    let secret_registry = Arc::new(codeterm_service_core::secrets::SecretRegistry::new());
    let transcribe_registry =
        Arc::new(codeterm_service_core::transcribe::TranscribeRegistry::new());
    let converter_registry = Arc::new(codeterm_plugin::converter::ConverterRegistry::new());
    let graph_node_registry = Arc::new(codeterm_plugin::graph_nodes::GraphNodeRegistry::default());
    let event_source_registry =
        Arc::new(codeterm_plugin::event_source::EventSourceRegistry::default());
    let graph_event_subscriptions = codeterm_plugin::graph_events::SubscriptionTable::new();
    let decision_model_registry =
        codeterm_service_core::service::supporter::registry::default_registry();
    let glance_registry: codeterm_service_core::glance::GlanceRegistryHandle =
        Arc::new(codeterm_service_core::glance::GlanceRegistry::new());
    let bubble_registry: codeterm_service_core::bubble::BubbleRegistryHandle =
        Arc::new(codeterm_service_core::bubble::BubbleRegistry::new());
    let repo_sink = Arc::new(codeterm_service_core::repowatch::TauriRepoEventSink::new(
        ui.clone(),
    ));
    let repo_watch: codeterm_service_core::repowatch::RepoWatchRegistryHandle = Arc::new(
        codeterm_service_core::repowatch::RepoWatchRegistry::new(repo_sink),
    );
    let view_registry: codeterm_service_core::view_host::ViewRegistryHandle =
        Arc::new(codeterm_service_core::view_host::ViewRegistry::new());
    let chatbackend_registry: codeterm_service_core::chatbackend::ChatBackendRegistryHandle =
        Arc::new(codeterm_service_core::chatbackend::ChatBackendRegistry::new());
    let chat_preprocessor_registry: codeterm_service_core::chatpreprocessor::ChatPreprocessorRegistryHandle =
    Arc::new(codeterm_service_core::chatpreprocessor::ChatPreprocessorRegistry::new());
    let agent_command_registry: codeterm_service_core::agent_commands::AgentCommandRegistryHandle =
        Arc::new(codeterm_service_core::agent_commands::AgentCommandRegistry::new());
    let webhook_registry: codeterm_plugin::WebhookRegistryHandle =
        Arc::new(codeterm_plugin::WebhookRegistry::new());
    let tab_menu_registry: codeterm_plugin::tab_menu::TabMenuRegistryHandle =
        Arc::new(codeterm_plugin::tab_menu::TabMenuRegistry::new());
    let surface_registry: codeterm_plugin::surfaces::SurfaceRegistryHandle =
        Arc::new(codeterm_plugin::surfaces::SurfaceRegistry::new());
    let cap_registries = plugin::js_runtime::CapabilityRegistries {
        decision_model: decision_model_registry.clone(),
        secret: Some(std::sync::Arc::new(
            codeterm_service_core::secrets::js_backend::AppSecretBackendRegistrar::new(
                secret_registry.clone(),
            ),
        )),
        transcribe: transcribe_registry.clone(),
        converter: converter_registry.clone(),
        graph_nodes: graph_node_registry.clone(),
        event_source: event_source_registry.clone(),
        glance: glance_registry.clone(),
        bubble: bubble_registry.clone(),
        view: view_registry.clone(),
        chatbackend: chatbackend_registry.clone(),
        chat_preprocessor: chat_preprocessor_registry.clone(),
        agent_command: agent_command_registry.clone(),
        webhook_receiver: webhook_registry.clone(),
        local_recording: Arc::new(
            codeterm_plugin::local_recording::LocalRecordingRegistry::default(),
        ),
        tab_menu: tab_menu_registry.clone(),
        surfaces: surface_registry.clone(),
    };
    let mut plugin_vms: Vec<Arc<plugin::js_runtime::PluginVm>> = Vec::new();
    let mut plugin_manager = plugin::PluginManager::new();
    plugin_manager.set_registrar(agent_registry.clone());
    plugin_manager.set_event_sink(states.event_broadcaster.clone());

    startup::forward_changes(ui.clone());
    {
        let mut sub = states.event_broadcaster.subscribe();
        let bridge_ui = ui.clone();
        let bridge_registry = agent_registry.clone();
        let bridge_chatbackend_registry = chatbackend_registry.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            while let Ok(ev) = sub.recv().await {
                if let codeterm_events::SystemEvent::ContributionsChanged { kind } = &ev.event {
                    bridge_ui.emit("contributions-changed", serde_json::json!({ "kind": kind }));
                    if kind == "agent_provider" {
                        codeterm_service_core::service::sync_watcher::install_bridge_rules_for_all(
                            &bridge_registry,
                        );
                        codeterm_service_core::service::mirror_skills_to_providers();
                        for provider in bridge_registry.all_provider_arcs() {
                            codeterm_service_core::agent::model_binary_watch::install(provider);
                        }
                        for provider in bridge_chatbackend_registry.active() {
                            codeterm_service_core::agent::model_binary_watch::install_chatbackend(
                                provider,
                                bridge_chatbackend_registry.clone(),
                            );
                        }
                    }
                }
            }
        });
    }

    plugin_manager
        .activate_builtin_native_plugins()
        .expect("built-in native plugin activation failed");
    log::info!("[startup] plugins activated: {:?}", plugin_manager.active());
    {
        let plugin_manager = std::sync::Arc::new(plugin_manager);
        let pm = plugin_manager.clone();
        let mut sub = states.event_broadcaster.subscribe();
        codeterm_service_core::async_runtime::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(ev) => pm.dispatch_event_type(&ev.frame.event_type),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    {
        if let Err(error) = codeterm_plugins::cleanup_stale_staging(codeterm_plugins::PluginCtx {
            data_root: codeterm_service_core::paths::data_dir(),
            version: codeterm_build_info::installed_version().map(str::to_string),
        }) {
            log::warn!("[plugin/bootstrap] staging cleanup failed: {error}");
        }
        match codeterm_plugins::retire_unshipped_system_plugins(codeterm_plugins::PluginCtx {
            data_root: codeterm_service_core::paths::data_dir(),
            version: codeterm_build_info::installed_version().map(str::to_string),
        }) {
            Ok(retired) if !retired.is_empty() => {
                log::info!("[plugin/retire] retired unshipped official plugins {retired:?}");
            }
            Ok(_) => {}
            Err(error) => log::warn!("[plugin/retire] official plugin retirement failed: {error}"),
        }
        let user_plugins = codeterm_service_core::paths::data_dir().join("plugins");
        let bootstrap_started = std::time::Instant::now();
        match plugin::embedded::bootstrap_into(&user_plugins) {
            Ok(written) if !written.is_empty() => {
                log::info!(
                    "[plugin/bootstrap] extracted embedded plugins {written:?} → {}",
                    user_plugins.display()
                );
            }
            Ok(_) => {
                log::debug!(
                    "[plugin/bootstrap] {} already has all system plugins",
                    user_plugins.display()
                );
            }
            Err(e) => {
                log::warn!(
                    "[plugin/bootstrap] failed to extract embedded plugins to {}: {e}",
                    user_plugins.display()
                );
            }
        }
        log::debug!(
            "[boot-operation] embedded plugin bootstrap/extraction elapsed_ms={}",
            bootstrap_started.elapsed().as_millis()
        );
    }
    {
        use codeterm_service_core::agent::provider::AgentProvider;
        use std::collections::HashSet;
        #[cfg(debug_assertions)]
        use std::path::PathBuf;
        agent_registry.begin_startup_provider_loading(
            codeterm_service_core::plugin::system_plugins::hashes()
                .iter()
                .map(|(id, _)| *id),
        );
        #[cfg(debug_assertions)]
        let workspace_root = Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.."
        )));
        #[cfg(not(debug_assertions))]
        let workspace_root: Option<std::path::PathBuf> = None;
        let roots = plugin::js_runtime::default_roots(workspace_root.as_deref());
        let (providers, report) = plugin::js_runtime::scan_roots(&roots);
        for outcome in &report {
            match outcome {
                plugin::js_runtime::LoadOutcome::Loaded {
                    plugin_id,
                    manifest_path,
                } => {
                    log::info!(
                        "[js-plugin] loaded {plugin_id} from {}",
                        manifest_path.display()
                    );
                }
                plugin::js_runtime::LoadOutcome::Skipped { path, reason } => {
                    log::warn!("[js-plugin] skipped {}: {reason}", path.display());
                }
            }
        }
        let keep_native: HashSet<String> = std::env::var("CODETERM_NATIVE_PROVIDERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let keep_all_native = keep_native.contains("all");
        for mut p in providers {
            let id = p.id().to_string();
            let version = p.manifest().version.clone();
            let trust = p.trust();
            let caps = p.manifest().capabilities.clone();
            plugin_vms.push(p.vm().clone());
            log::info!(
            "[js-plugin] {id} version={version} trust={:?} caps(agent={} secret={} transcribe={})",
            trust,
            caps.agent_provider,
            caps.secret_backend.enabled(),
            caps.transcriber
        );
            let core = plugin::js_runtime::loader::is_core_plugin(p.manifest(), trust);
            let registration_started = std::time::Instant::now();
            cap_registries.register_plugin_with_core(p.manifest(), p.vm(), core);
            log::debug!("[boot-operation] capability registration plugin={} including_binary_resolution_fingerprint elapsed_ms={}", id, registration_started.elapsed().as_millis());
            if !caps.agent_provider {
                plugin::audit::append_daemon_reloaded(&id, &version);
                continue;
            }

            if keep_all_native || keep_native.contains(&id) {
                if !agent_registry.register_provider_if_absent(Box::new(p)) {
                    log::info!(
                        "[js-plugin] {id} held back — \
                    CODETERM_NATIVE_PROVIDERS pins the native impl"
                    );
                } else {
                    plugin::audit::append_daemon_reloaded(&id, &version);
                }
            } else {
                p.set_registrar(agent_registry.clone());
                log::info!(
                    "[js-plugin] {id} registered as authoritative \
                (set CODETERM_NATIVE_PROVIDERS={id} to rollback)"
                );
                agent_registry.register_provider(Box::new(p));
                plugin::audit::append_daemon_reloaded(&id, &version);
            }
        }
        agent_registry.finish_startup_provider_loading();
        let watcher = plugin::js_runtime::HotReloadRegistry::new(
            roots.clone(),
            agent_registry.clone(),
            cap_registries.clone(),
        );
        watcher.seed();
        for vm in &plugin_vms {
            watcher.register_vm(vm.plugin_id(), vm);
        }
        watcher.start();
        log::info!(
            "[js-plugin] hot-reload watcher started on {} roots",
            roots.len()
        );
    }

    agent::model_catalog::refresh_example_files();

    startup::refresh_providers(&agent_registry);
    api.complete("providers");

    let prompt_detector = Arc::new(Mutex::new(agent::prompt::PromptDetector::new()));
    let (bridge_outbound_tx, bridge_outbound_rx) = bridge::manager::create_outbound_channel();

    let startup_step = startup::StartupStep::logged("memory initialization");
    let mem = mem::MemManager::open(&codeterm_service_core::paths::data_dir())
        .expect("mem manager init failed");
    match mem.seed_bundled_if_needed() {
        Ok(outcome) if outcome.written() > 0 || !outcome.skipped.is_empty() => {
            log::info!(
            "[mem] bundled seed: {} written ({} created, {} bootstrapped, {} refreshed), {} skipped",
            outcome.written(),
            outcome.created,
            outcome.bootstrapped,
            outcome.refreshed,
            outcome.skipped.len()
        );
            for (title, reason) in &outcome.skipped {
                log::info!("[mem] bundled seed skip: {title} ({reason:?})");
            }
        }
        Ok(_) => log::info!("[mem] starter items already seeded"),
        Err(e) => log::warn!("[mem] seed failed: {}", e),
    }

    codeterm_service_core::service::general_agent::sync_identity_to_mem(&mem);
    mem.reindex_missing();
    drop(startup_step);

    let startup_step = startup::StartupStep::logged("service state construction");
    let svc_ctx = codeterm_service_core::service::build_service_context(
        &states,
        ui.clone(),
        shell,
        codeterm_service_core::async_runtime::handle().clone(),
        agent_registry,
        &chatbackend_registry,
        mem,
        prompt_detector,
        bridge_outbound_tx,
        bridge_outbound_rx,
        &secret_registry,
        &transcribe_registry,
        &converter_registry,
        &graph_node_registry,
        &event_source_registry,
        &graph_event_subscriptions,
        decision_model_registry,
        &glance_registry,
        &bubble_registry,
        repo_watch,
        &view_registry,
        &chat_preprocessor_registry,
        &agent_command_registry,
        &webhook_registry,
        &tab_menu_registry,
        surface_registry,
        &cap_registries,
    );
    codeterm_service_core::service::supporter::runtime_with_notifications(&svc_ctx);
    let service_state: ServiceState = svc_ctx;
    if let Some(observer) = worker_observer {
        service_state
            .ext::<Arc<mem::supervisor::Supervisor>>()
            .observe_spawns(observer);
    }
    codeterm_service_core::mem_ops::embedder::start(&service_state);
    codeterm_service_core::ports::orchestration::install(&service_state);
    service_state
        .chat_send_ledger
        .bind_settlement_owner(&service_state);
    {
        let service = service_state.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            if service.analytics_worker().await.is_err() {
                log::debug!("[analytics] initialization unavailable");
            }
        });
    }
    service_state.plan_store.bind_task_key_port(Arc::new(
        codeterm_service_core::service::plan::task_key_port_impl::MuxTaskKeyPort::new(
            service_state.mux.clone(),
            Arc::downgrade(&service_state),
        ),
    ));
    service_state
        .mem
        .extraction_authority()
        .set_cancel_sink(Arc::new(codeterm_service_core::mem_host::WorkerCancelSink(
            service_state.clone(),
        )));
    {
        let config = states.config.lock().clone();
        if config.general_agent.enabled && config.general_agent.first_run_complete {
            let default_chat_view =
                codeterm_service_core::service::general_agent::default_chat_view_for_provider(
                    &config,
                    &service_state.agent_registry,
                    &config.general_agent.agent_provider,
                );
            codeterm_service_core::service::general_agent::apply_general_chat_view_default(
                &mut service_state.mux.write(),
                default_chat_view,
            );
        }
    }
    {
        let host: Arc<dyn codeterm_plugin::PluginHostCtx> = service_state.clone();
        for vm in &plugin_vms {
            vm.set_service(Arc::downgrade(&host));
        }
        plugin::js_runtime::set_watcher_service(Arc::downgrade(&host));
        codeterm_service_core::service::plugin_prompt::refresh(
            service_state
                .ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>(),
        );
        codeterm_service_core::service::plugin_skills::spawn_sync(service_state.mem.clone());
        for provider in chatbackend_registry.active() {
            codeterm_service_core::agent::model_binary_watch::install_chatbackend(
                provider,
                chatbackend_registry.clone(),
            );
        }
        let _ = chatbackend_registry.start_discovery();
    }
    let _ = codeterm_service_core::async_runtime::block_on(service_state.billing());
    service_state.sync_limit_gate();
    codeterm_service_core::service::agent_runtime::restore_all(&service_state);
    codeterm_service_core::service::lazy_restore::initialize(
        &service_state,
        states.config.lock().restore.active_window_hours,
    );

    {
        let service = service_state.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            let _ = service
                .refresh_billing(codeterm_billing::RefreshEvent::SignIn)
                .await;
        });
    }
    startup::reconcile_defaults(&service_state);
    let lane_watch_service = service_state.clone();
    codeterm_service_core::async_runtime::spawn_blocking(move || {
        lane_watch_service.restore_lane_watches();
    });
    codeterm_service_core::delayed_send::start_scheduler(service_state.clone());
    codeterm_service_core::service::start_plan_reminder_scheduler(service_state.clone());
    drop(startup_step);

    api.complete("service");
    let plan_started = std::time::Instant::now();
    let startup_step = startup::StartupStep::logged("restored plan loading");
    startup::restore_plans(&service_state);
    drop(startup_step);
    log::debug!(
        "[boot-operation] restored plan loading elapsed_ms={}",
        plan_started.elapsed().as_millis()
    );
    api.complete("plans");

    startup::background_init::initialize(&service_state);

    api.complete("background");
    let platform_started = std::time::Instant::now();
    let startup_step = startup::StartupStep::logged("platform and server initialization");

    startup::install_bundled_cli(service_state.shell.as_ref());
    drop(startup::restore_complete(&service_state));
    let bundles = startup::start_servers(
        &states,
        &service_state,
        frontend_dev_url,
        resource_dir,
        quit_port,
    )?;
    service_state.schedule_restored_agent_delivery_reconciliation();
    codeterm_service_core::service::provider_hooks::ensure_integrations_for_all(
        &service_state.agent_registry,
        &states.config,
        service_state.clone(),
    );
    let (api_port, allow_non_prod_updates) = {
        let config = states.config.lock();
        (
            config.api.port,
            config.updater.allow_provider_updates_in_non_prod,
        )
    };
    service::check_provider_update_available(
        service_state.clone(),
        api_port,
        allow_non_prod_updates,
    );
    if states.config.lock().api.enabled {
        codeterm_service_core::start_api_liveness_watchdog(
            service_state.ui.clone(),
            states.mesh.clone(),
            states.rest_api.clone(),
            states.config.lock().api.port,
        );
    }
    codeterm_service_core::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            if let Err(error) =
                tokio::task::spawn_blocking(codeterm_service_core::crash::lifecycle::heartbeat)
                    .await
            {
                log::warn!("[recovery] heartbeat task failed: {error}");
            }
        }
    });
    {
        let ui = service_state.ui.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            let new_reports = codeterm_service_core::crash::check_previous_crashes();
            if !new_reports.is_empty() {
                let dir = codeterm_service_core::crash::report::crashes_dir();
                let files: Vec<String> = new_reports
                    .iter()
                    .filter_map(|p| p.file_name())
                    .filter_map(|n| n.to_str())
                    .map(|s| s.to_string())
                    .collect();
                log::warn!(
                    "[crash] previous-session reports found: {} (dir={})",
                    files.len(),
                    dir.display()
                );
                ui.emit(
                    "crash_report_available",
                    serde_json::json!({
                        "dir": dir.to_string_lossy(),
                        "files": files,
                        "recovery": codeterm_service_core::crash::lifecycle::recovery_state(),
                    }),
                );
            }
        });
    }

    let overlays = service::persona::overlay::reload();
    for rejection in overlays.rejected() {
        log::warn!(
            "[role-preset] refused {} [{}] {}",
            rejection.path,
            rejection.reason.code(),
            rejection.reason.detail()
        );
    }
    service::sync_watcher::start(
        codeterm_service_core::paths::data_dir(),
        service_state.agent_registry.clone(),
    );
    startup::start_background_tasks(&service_state, &states, bundles.clone());
    agent::state_reactor::start(service_state.clone());
    agent::live_state::start(service_state.clone());
    drop(startup_step);
    log::debug!(
        "[boot-operation] platform and server initialization elapsed_ms={}",
        platform_started.elapsed().as_millis()
    );
    api.complete("platform");

    Ok(ComposedService {
        states,
        service: service_state,
        frontend_bundles: bundles,
    })
}
