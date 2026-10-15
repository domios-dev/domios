use crate::startup;
use crate::{crash, keepawake};
use codeterm_service_core::service::ServiceState;
use codeterm_service_core::{agent, bridge, mem, plugin, pty, service};
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::Manager;

pub fn run(
    context: tauri::Context<codeterm_service_core::app_runtime::Runtime>,
    register_commands: fn(
        tauri::Builder<codeterm_service_core::app_runtime::Runtime>,
    ) -> tauri::Builder<codeterm_service_core::app_runtime::Runtime>,
) {
    codeterm_service_core::async_runtime::install(tauri::async_runtime::handle().inner().clone());
    // Never automatic: only an explicit `repair-user-path` argv reaches it.
    startup::run_user_path_repair_if_invoked();

    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init, otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    // crash originates (tokio worker, Tauri main, FFI thread, signal handler).
    crash::install();

    startup::setup_logging();

    log::info!("CodeTerm starting...");
    let _instance_lifetime = match codeterm_service_core::acquire_instance_lifetime() {
        Ok(lifetime) => lifetime,
        Err(error) => {
            log::error!("[startup] could not acquire instance lifetime lock: {error}");
            return;
        }
    };
    let startup_recovery = crash::record_startup();
    if let Some(record) = &startup_recovery.last_crash {
        log::warn!(
            "[recovery] previous process classified as {:?}; old_pid={} old_epoch={} new_epoch={} last_healthy_at={} backoff_ms={} loop_protected={}",
            record.classification,
            record.previous.pid,
            record.previous.epoch,
            record.new_epoch,
            record.previous.last_healthy_at,
            record.supervisor_action.backoff_ms,
            record.supervisor_action.loop_protected
        );
    }

    // Lift the open-file soft limit before anything opens fds (#186).
    codeterm_service_core::raise_fd_limit();

    // Reap msedgewebview2.exe orphaned by a prior hard GUI crash, before window init (GH #183).
    codeterm_service_core::reap_orphaned_webview2();

    // Write shell integration scripts (OSC 7 CWD tracking)
    pty::shell_integration::ensure_scripts();

    let states = startup::init_states();

    // Seed canonical ~/.codeterm/skills/, then mirror into provider dirs agents load from (#178).
    codeterm_service_core::service::seed_bundled_skills();
    codeterm_service_core::service::mirror_skills_to_providers();

    let mut builder = codeterm_service_core::app_runtime::builder()
        .plugin(tauri_plugin_shell::init())
        .plugin(startup::native_notification())
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

    if !cfg!(debug_assertions) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }

    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .manage(states.storage.clone())
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
        .manage(states.resize_owner.clone())
        .manage(states.rest_api.clone());
    register_commands(builder)
        .setup(move |app| {
            #[cfg(debug_assertions)]
            tauri::async_runtime::spawn(codeterm_service_core::startup::runtime_diagnostics::watch("main"));
            {
                let api_config = states.config.lock().api.clone();
                let window = startup::frontend_shell_create_window(app, &api_config)?;
                startup::setup_window_styling(&window);
                #[cfg(windows)]
                codeterm_service_core::install_native_resize(&window)?;
                codeterm_service_core::spawn_webview_watchdog(window);
            }

            // Create unified service context (available to both API and Tauri commands)
            let app_handle = app.handle().clone();

            // remainder; timed + already expired → drop the snapshot.
            keepawake::reap_orphaned_children_on_startup();
            keepawake::resume_from_disk(&states.keepawake, &app_handle);

            agent::model_catalog::ensure_readme_installed();
            // The retired native backend's master-password drop has no reader.
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
            // Carry a pre-existing native Bitwarden setup into the plugin's namespace (native Bitwarden is retired).
            {
                let mut cfg = states.config.lock();
                if codeterm_service_core::secrets::migrate_native_bitwarden_to_plugin(&mut cfg) {
                    let snapshot = cfg.clone();
                    drop(cfg);
                    let _ = codeterm_service_core::config::save_config_pub(&snapshot);
                }
            }

            let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
            // Capability registries — populated as JS plugins load, then handed to ServiceContext.
            let secret_registry = Arc::new(codeterm_service_core::secrets::SecretRegistry::new());
            let transcribe_registry = Arc::new(codeterm_service_core::transcribe::TranscribeRegistry::new());
            let converter_registry = Arc::new(codeterm_plugin::converter::ConverterRegistry::new());
            let graph_node_registry = Arc::new(codeterm_plugin::graph_nodes::GraphNodeRegistry::default());
            let event_source_registry = Arc::new(codeterm_plugin::event_source::EventSourceRegistry::default());
            let graph_event_subscriptions = codeterm_plugin::graph_events::SubscriptionTable::new();
            let decision_model_registry = codeterm_service_core::service::supporter::registry::default_registry();
            let glance_registry: codeterm_service_core::glance::GlanceRegistryHandle =
                Arc::new(codeterm_service_core::glance::GlanceRegistry::new());
            let bubble_registry: codeterm_service_core::bubble::BubbleRegistryHandle =
                Arc::new(codeterm_service_core::bubble::BubbleRegistry::new());
            let repo_watch: codeterm_service_core::repowatch::RepoWatchRegistryHandle =
                Arc::new(codeterm_service_core::repowatch::RepoWatchRegistry::new(Arc::new(
                    codeterm_service_core::repowatch::TauriRepoEventSink::new(app_handle.clone()),
                )));
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
            // One bundle of registry handles, shared by the startup loop and the hot-reload watcher (same register_plugin path, no drift).
            let cap_registries = plugin::js_runtime::CapabilityRegistries {
                decision_model: decision_model_registry.clone(),
                secret: Some(std::sync::Arc::new(
                    codeterm_service_core::secrets::js_backend::AppSecretBackendRegistrar::new(secret_registry.clone())
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
                local_recording: Arc::new(codeterm_plugin::local_recording::LocalRecordingRegistry::default()),
                tab_menu: tab_menu_registry.clone(),
                surfaces: surface_registry.clone(),
            };
            // VMs collected as they load, so the `ServiceContext` can be late-bound into each sandbox once it's built below.
            let mut plugin_vms: Vec<Arc<plugin::js_runtime::PluginVm>> = Vec::new();
            let mut plugin_manager = plugin::PluginManager::new();
            plugin_manager.set_registrar(agent_registry.clone());
            plugin_manager.set_event_sink(states.event_broadcaster.clone());

            startup::model_catalogue::forward_changes(&app_handle);

            // Bridge ContributionsChanged → frontend event + mem bridge rule refresh.
            {
                use tauri::Emitter;
                let mut sub = states.event_broadcaster.subscribe();
                let bridge_handle = app_handle.clone();
                let bridge_registry = agent_registry.clone();
                let bridge_chatbackend_registry = chatbackend_registry.clone();
                tauri::async_runtime::spawn(async move {
                    while let Ok(ev) = sub.recv().await {
                        if let codeterm_events::SystemEvent::ContributionsChanged { kind } =
                            &ev.event
                        {
                            let _ = bridge_handle.emit(
                                "contributions-changed",
                                serde_json::json!({ "kind": kind }),
                            );
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

            {
                use plugin::agent_claude::AgentClaudePlugin;
                let manifest = AgentClaudePlugin::manifest();
                plugin_manager.register(manifest);
                plugin_manager.attach_native(Box::new(AgentClaudePlugin::new()))
                    .expect("agent-claude plugin attach failed");
                plugin_manager.activate(plugin::agent_claude::PLUGIN_ID)
                    .expect("agent-claude plugin activation failed");
            }
            // codex + opencode are JS plugins now (no native bundled providers).
            log::info!("[startup] plugins activated: {:?}", plugin_manager.active());

            // Fan SystemEvents to active plugins' on_event; keep the manager alive in the subscriber task. Exits on bus close.
            {
                let plugin_manager = std::sync::Arc::new(plugin_manager);
                let pm = plugin_manager.clone();
                let mut sub = states.event_broadcaster.subscribe();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match sub.recv().await {
                            Ok(ev) => pm.dispatch_event_type(ev.event.type_name()),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // Extract embedded providers on first run for offline/dev-limited setups.
            // Skip in CODETERM_DEV=1 so workspace sources stay authoritative.
            if std::env::var("CODETERM_DEV").as_deref() != Ok("1") {
                let user_plugins = codeterm_service_core::paths::data_dir().join("plugins");
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
            } else {
                log::info!("[plugin/bootstrap] CODETERM_DEV=1 — skipping embedded extract, workspace plugins are authoritative");
            }

            // Register JS plugins from workspace and user drop-in dirs.
            // JS provider defaults to authoritative on collision; set CODETERM_NATIVE_PROVIDERS to pin native.
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
                let workspace_root = Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
                #[cfg(not(debug_assertions))]
                let workspace_root: Option<std::path::PathBuf> = None;
                let roots = plugin::js_runtime::default_roots(workspace_root.as_deref());
                let (providers, report) = plugin::js_runtime::scan_roots(&roots);
                for outcome in &report {
                    match outcome {
                        plugin::js_runtime::LoadOutcome::Loaded { plugin_id, manifest_path } => {
                            log::info!(
                                "[js-plugin] loaded {plugin_id} from {}",
                                manifest_path.display()
                            );
                        }
                        plugin::js_runtime::LoadOutcome::Skipped { path, reason } => {
                            log::warn!(
                                "[js-plugin] skipped {}: {reason}",
                                path.display()
                            );
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
                    // Hold a VM handle so the ServiceContext can be late-bound into its sandbox after service init.
                    plugin_vms.push(p.vm().clone());
                    log::info!(
                        "[js-plugin] {id} trust={:?} caps(agent={} secret={} transcribe={})",
                        trust,
                        caps.agent_provider,
                        caps.secret_backend.enabled(),
                        caps.transcriber
                    );

                    // Same register_plugin seam as the hot-reload watcher; a capability-only plugin is NOT registered as an agent.
                    let core = plugin::js_runtime::loader::is_core_plugin(p.manifest(), trust);
                    cap_registries.register_plugin_with_core(p.manifest(), p.vm(), core);
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
                        // JS takes authority; the displaced native impl is preserved as a fallback for unexported methods.
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

                // Hot-reload keeps plugin VMs in sync: reload existing plugins and drop vanished plugin dirs.
                let watcher = plugin::js_runtime::HotReloadRegistry::new(
                    roots.clone(),
                    agent_registry.clone(),
                    cap_registries.clone(),
                );
                // Seed tracked-state + mark already-registered plugins activated-at-startup, so the first tick doesn't re-register them.
                watcher.seed();
                // Track each plugin's live VM so a later config-only edit re-stamps settings in place instead of rebuilding.
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

            startup::model_catalogue::refresh_providers(&agent_registry);

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

            // Re-index any items missing vectors (catches failed bg indexing)
            mem.reindex_missing();
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("service state construction");
            let svc_ctx = codeterm_service_core::service::build_service_context(
                &states,
                &app_handle,
                Arc::new(crate::host_ports::TauriUiEvents(app_handle.clone())),
                Arc::new(crate::host_ports::TauriShell(app_handle.clone())),
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
            let service_state: ServiceState = Arc::new(svc_ctx);
            codeterm_service_core::ports::orchestration::install(&service_state);
            service_state.chat_send_ledger.bind_settlement_owner(&service_state);
            {
                let service = service_state.clone();
                tauri::async_runtime::spawn(async move {
                    if service.analytics_worker().await.is_err() {
                        log::debug!("[analytics] initialization unavailable");
                    }
                });
            }
            service_state.plan_store.bind_task_key_port(Arc::new(
                codeterm_service_core::service::plan::task_key_port_impl::MuxTaskKeyPort::new(
                    service_state.mux.clone(),
                    service_state.app_handle.clone(),
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
            // Late-bind the ServiceContext into every loaded plugin's sandbox. Weak so VMs never keep the service alive past teardown.
            {
                let host: Arc<dyn codeterm_plugin::PluginHostCtx> = service_state.clone();
                for vm in &plugin_vms {
                    vm.set_service(Arc::downgrade(&host));
                }
                plugin::js_runtime::set_watcher_service(Arc::downgrade(&host));
                codeterm_service_core::service::plugin_prompt::refresh(service_state.ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>());
                codeterm_service_core::service::plugin_skills::spawn_sync(service_state.mem.clone(), service_state.ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>().clone());
                for provider in chatbackend_registry.active() {
                    codeterm_service_core::agent::model_binary_watch::install_chatbackend(
                        provider,
                        chatbackend_registry.clone(),
                    );
                }
                let _ = chatbackend_registry.start_discovery();
            }
            codeterm_service_core::service::agent_runtime::restore_all(&service_state);
            codeterm_service_core::service::lazy_restore::initialize(&service_state, states.config.lock().restore.active_window_hours);
            app.manage(service_state.clone());
            codeterm_service_core::service::account::register_deep_links(&app_handle, service_state.clone())?;
            {
                let service = service_state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = service.refresh_billing(codeterm_billing::RefreshEvent::SignIn).await;
                });
            }
            startup::model_catalogue::reconcile_defaults(&service_state);
            let lane_watch_service = service_state.clone();
            tauri::async_runtime::spawn_blocking(move || {
                lane_watch_service.restore_lane_watches();
            });
            codeterm_service_core::delayed_send::start_scheduler(service_state.clone());
            codeterm_service_core::service::start_plan_reminder_scheduler(service_state.clone());
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("restored plan loading");
            startup::restore_plans(&service_state);
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("background service initialization");
            {
                let store = service_state.services_store.clone();
                let activity = codeterm_service_core::service::activity::ActivityWriter::new(store.conn());
                codeterm_service_core::service::health::scheduler::start(store, activity, app.handle().clone());
            }

            // Secrets init happens before API listener binds to avoid auth calls hitting uninitialized state.

            // Librarian: hourly file-sync + skill-folder scan, run as a subprocess via the supervisor so a panic / OOM cannot take the daemon down. First fire after a 30s warm-up.
            {
                let supervisor = service_state.ext::<Arc<codeterm_service_core::mem::supervisor::Supervisor>>().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    loop {
                        match supervisor.run(codeterm_service_core::mem::supervisor::WorkerKind::LibrarianOnce).await {
                            Ok(report) => log::info!("[librarian] {}", report),
                            Err(e) => log::warn!("[librarian] {}", e),
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    }
                });
            }

            {
                let mem = service_state.mem.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(45)).await;
                    let mem_for_build = mem.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        if mem_for_build.layout_needs_rebuild() {
                            match mem_for_build.run_full_layout() {
                                Ok(v) => log::info!("[graph_layout] startup build v{v}"),
                                Err(e) => log::warn!("[graph_layout] startup build: {e}"),
                            }
                        }
                    })
                    .await;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        mem.layout_flush_if_due();
                    }
                });
            }

            // Resume unfinished extraction only when auto-extract is on; `mem extract-pending` stays the manual path.
            if codeterm_service_core::config::memory_config().auto_extract {
                let svc_for_extract = service_state.clone();
                let queued = codeterm_service_core::mem_host::resume_extract_pending(
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

            {
                let log_store = codeterm_service_core::logs::LogStore::open(&codeterm_service_core::paths::data_dir())
                    .expect("app_logs.db init");
                log_store.attach_app(app_handle.clone());
                codeterm_service_core::logs::set_global(log_store.clone());
                log_store.start_flush_loop();
            }

            {
                let poller = codeterm_service_core::usage::UsagePoller::new(
                    (*service_state).clone(),
                    service_state.usage_store.clone(),
                    service_state.ext::<codeterm_service_core::usage::TabActivity>().clone(),
                );
                *service_state.ext::<codeterm_service_core::usage::poller::UsagePollerSlot>().0.lock() = Some(poller.clone());
                poller.spawn();
            }
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("platform and server initialization");
            startup::setup_conpty_host(app);
            startup::install_bundled_cli(app);
            drop(startup::restore_complete(&service_state));
            startup::start_servers(app, &states, &service_state);
            startup::frontend_shell_start(app_handle.clone(), states.rest_api.clone());
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
                    app_handle.clone(),
                    states.mesh.clone(),
                    states.rest_api.clone(),
                    states.config.lock().api.port,
                );
            }
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Err(error) = tokio::task::spawn_blocking(codeterm_service_core::crash::lifecycle::heartbeat).await {
                        log::warn!("[recovery] heartbeat task failed: {error}");
                    }
                }
            });
            startup::build_system_tray(app)?;

            // macOS quits never emit ExitRequested; observe AppKit's terminate instead.
            #[cfg(target_os = "macos")]
            crash::macos_quit::install_quit_hook();

            // Catch SIGTERM/SIGINT: run the same graceful cleanup as a Tauri Exit, then exit cleanly. (Windows is covered above.)
            #[cfg(unix)]
            {
                let signal_app = app_handle.clone();
                crash::shutdown::install_unix_signal_handlers(move || {
                    crash::exit::graceful_exit_cleanup(&signal_app);
                });
            }

            // the React side has time to attach the listener.
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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
                        codeterm_service_core::events::emit_everywhere(
                            &app_handle,
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
            service::sync_watcher::start(codeterm_service_core::paths::data_dir(), service_state.agent_registry.clone());
            startup::start_background_tasks(&app_handle, &states);
            // Tab-state reactor: classify parked tabs and auto-advance via the provider's recommendation.
            agent::state_reactor::start(service_state.clone());
            // working→idle edge (no output to ride on when a turn settles).
            agent::live_state::start(service_state.clone());
            drop(startup_step);

            // PTY read loops are started by spawn_pty command (initiated by frontend)
            Ok(())
        })
        .on_window_event(startup::handle_window_event)
        .build(startup::frontend_shell_application_context(context))
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

            // Mark shutting down the moment exit is requested, so the liveness watchdog doesn't read the imminent API-down as a crash.
            if let tauri::RunEvent::ExitRequested { .. } = _event {
                log::info!("[shutdown] RunEvent::ExitRequested received");
                crash::shutdown::mark_shutting_down();
            }

            // log doesn't survive into the next run. The updater's graceful_shutdown already does this on the in-place upgrade path; this catches every other shutdown route.
            if let tauri::RunEvent::Exit = _event {
                crash::exit::handle_run_exit(_app);
            }
        });
    log::info!("[shutdown] Tauri event loop returned");
}
