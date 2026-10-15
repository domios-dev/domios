pub mod agent;
pub mod agent_commands;
pub mod api_support;
pub mod app_runtime;
pub use codeterm_util::attachments;
mod autostart;
pub use autostart::{
    disable as disable_autostart, enable as enable_autostart, is_enabled as autostart_is_enabled,
};
pub mod bridge;
pub mod bridge_host;
pub mod bubble;
pub mod chatbackend;
pub mod chatpipeline;
pub mod chatpreprocessor;
pub mod config;
pub mod crash;
pub mod cron;
pub mod delayed_send;
pub mod dispatch;
mod fdlimit;
mod flow_trace;
pub mod frontend_bundle_api_version;
pub mod glance;
mod instance_lifetime;
pub mod keepawake;
mod liveness;
pub mod lock_hold;
pub mod logs;
pub mod mem;
pub mod mem_host;
pub mod mem_ops;
pub mod mesh;
pub mod mesh_host;
pub mod message_markers;
pub mod multiplexer;
#[cfg(windows)]
mod native_resize;
pub mod notification;
pub mod pasteboard;
pub mod paths;
pub mod sentinel;
pub use codeterm_util::persisted_state;
pub mod plan;
pub mod plugin;
pub mod ports;
pub mod process_util;
pub mod prompts;
pub mod provider;
pub mod pty;
pub mod repowatch;
pub mod resize_owner;
pub mod runaway_sweeper;
pub mod secrets;
pub mod service;
pub mod session;
pub mod shell;
pub mod startup;
pub mod storage;
#[cfg(test)]
mod telegram269_wire_contract;
pub mod transcribe;
pub mod transcribe_adapter;
pub mod update_state;
pub mod usage;
pub mod user_last_activity;
pub mod view;
pub mod view_host;
mod webview2_reap;
mod webview_watchdog;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use multiplexer::commands::MuxState;
pub use notification::NotifState;
use pty::manager::{OutputBuffer, PtyManager};
use pty::screen_state::ScreenState;
use service::{ServiceContext, ServiceState};

pub type PtyState = Arc<lock_hold::TimedMutex<PtyManager>>;
pub type OutputBufferState = Arc<lock_hold::TimedMutex<OutputBuffer>>;
pub type ScreenStateHandle = Arc<lock_hold::TimedMutex<ScreenState>>;
pub type AgentTrackerState = Arc<lock_hold::TimedMutex<agent::status::TabStatusTracker>>;
pub type ShellCapabilityState = Arc<Mutex<agent::command_detection::ShellCapabilityTracker>>;
pub type ReportStoreState = Arc<Mutex<agent::reports::ReportStore>>;
pub type PromptDetectorState = Arc<Mutex<agent::prompt::PromptDetector>>;
pub use cron::scheduler::{CronState, CronWake};
pub type UpdateStateHandle = Arc<Mutex<update_state::UpdateState>>;
pub type MeshStateHandle = mesh::MeshState;
pub type KeepAwakeStateHandle = keepawake::KeepAwakeState;
pub type ResizeOwnerState = resize_owner::ResizeOwnerState;

pub use startup::{spawn_telegram_bridge, start_telegram_bridge};

pub fn run(
    context: tauri::Context<crate::app_runtime::Runtime>,
    register_commands: fn(
        tauri::Builder<crate::app_runtime::Runtime>,
    ) -> tauri::Builder<crate::app_runtime::Runtime>,
) {
    // Never automatic: only an explicit `repair-user-path` argv reaches it.
    startup::run_user_path_repair_if_invoked();

    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init, otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    // crash originates (tokio worker, Tauri main, FFI thread, signal handler).
    crash::install();

    startup::setup_logging();

    log::info!("CodeTerm starting...");
    let _instance_lifetime = match instance_lifetime::acquire() {
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
    fdlimit::raise_fd_limit();

    // Reap msedgewebview2.exe orphaned by a prior hard GUI crash, before window init (GH #183).
    webview2_reap::reap_orphaned_webview2();

    // Write shell integration scripts (OSC 7 CWD tracking)
    pty::shell_integration::ensure_scripts();

    let states = startup::init_states();

    // Seed canonical ~/.codeterm/skills/, then mirror into provider dirs agents load from (#178).
    crate::service::skill_mirror::seed_bundled_skills();
    crate::service::skill_mirror::mirror_skills_to_providers();

    let mut builder = crate::app_runtime::builder()
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
            {
                let api_config = states.config.lock().api.clone();
                let window = startup::frontend_shell::create_window(app, &api_config)?;
                startup::setup_window_styling(&window);
                #[cfg(windows)]
                native_resize::install(&window)?;
                webview_watchdog::spawn(window);
            }

            // Create unified service context (available to both API and Tauri commands)
            let app_handle = app.handle().clone();

            // remainder; timed + already expired → drop the snapshot.
            keepawake::reap_orphaned_children_on_startup();
            keepawake::resume_from_disk(&states.keepawake, &app_handle);

            agent::model_catalog::ensure_readme_installed();
            // The retired native backend's master-password drop has no reader.
            match crate::secrets::purge_legacy_master_password_file() {
                crate::secrets::LegacyPurge::Absent => {}
                crate::secrets::LegacyPurge::SecurelyErased => {
                    log::info!("[secrets] securely erased the retired master-password drop")
                }
                crate::secrets::LegacyPurge::RemovedWithoutOverwrite => log::warn!(
                    "[secrets] removed the retired master-password drop WITHOUT overwriting it; the old secret may still be recoverable from disk — rotate it"
                ),
                crate::secrets::LegacyPurge::RemovalFailed { overwritten } => log::warn!(
                    "[secrets] the retired master-password drop is still on disk (overwritten={overwritten}); it may still be recoverable — remove and rotate it"
                ),
                crate::secrets::LegacyPurge::InspectionFailed => log::warn!(
                    "[secrets] could not inspect the retired master-password drop; assume it is present and recoverable until checked"
                ),
            }
            // Carry a pre-existing native Bitwarden setup into the plugin's namespace (native Bitwarden is retired).
            {
                let mut cfg = states.config.lock();
                if crate::secrets::migrate_native_bitwarden_to_plugin(&mut cfg) {
                    let snapshot = cfg.clone();
                    drop(cfg);
                    let _ = crate::config::save_config_pub(&snapshot);
                }
            }

            let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
            // Capability registries — populated as JS plugins load, then handed to ServiceContext.
            let secret_registry = Arc::new(crate::secrets::SecretRegistry::new());
            let transcribe_registry = Arc::new(crate::transcribe::TranscribeRegistry::new());
            let converter_registry = Arc::new(codeterm_plugin::converter::ConverterRegistry::new());
            let graph_node_registry = Arc::new(codeterm_plugin::graph_nodes::GraphNodeRegistry::default());
            let event_source_registry = Arc::new(codeterm_plugin::event_source::EventSourceRegistry::default());
            let graph_event_subscriptions = codeterm_plugin::graph_events::SubscriptionTable::new();
            let decision_model_registry = crate::service::supporter::registry::default_registry();
            let glance_registry: crate::glance::GlanceRegistryHandle =
                Arc::new(crate::glance::GlanceRegistry::new());
            let bubble_registry: crate::bubble::BubbleRegistryHandle =
                Arc::new(crate::bubble::BubbleRegistry::new());
            let repo_watch: crate::repowatch::RepoWatchRegistryHandle =
                Arc::new(crate::repowatch::RepoWatchRegistry::new(Arc::new(
                    crate::repowatch::TauriRepoEventSink::new(app_handle.clone()),
                )));
            let view_registry: crate::view_host::ViewRegistryHandle =
                Arc::new(crate::view_host::ViewRegistry::new());
            let chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle =
                Arc::new(crate::chatbackend::ChatBackendRegistry::new());
            let chat_preprocessor_registry: crate::chatpreprocessor::ChatPreprocessorRegistryHandle =
                Arc::new(crate::chatpreprocessor::ChatPreprocessorRegistry::new());
            let agent_command_registry: crate::agent_commands::AgentCommandRegistryHandle =
                Arc::new(crate::agent_commands::AgentCommandRegistry::new());
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
                    crate::secrets::js_backend::AppSecretBackendRegistrar::new(secret_registry.clone())
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
                                crate::service::sync_watcher::install_bridge_rules_for_all(
                                    &bridge_registry,
                                );
                                crate::service::skill_mirror::mirror_skills_to_providers();
                                for provider in bridge_registry.all_provider_arcs() {
                                    crate::agent::model_binary_watch::install(provider);
                                }
                                for provider in bridge_chatbackend_registry.active() {
                                    crate::agent::model_binary_watch::install_chatbackend(
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
                let user_plugins = crate::paths::data_dir().join("plugins");
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
                use crate::agent::provider::AgentProvider;
                use std::collections::HashSet;
                #[cfg(debug_assertions)]
                use std::path::PathBuf;
                agent_registry.begin_startup_provider_loading(
                    crate::plugin::system_plugins::SYSTEM_PLUGIN_HASHES
                        .iter()
                        .map(|(id, _)| *id),
                );
                #[cfg(debug_assertions)]
                let workspace_root = Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")));
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
            let mem = mem::MemManager::open(&crate::paths::data_dir())
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

            crate::service::general_agent::sync_identity_to_mem(&mem);

            // Re-index any items missing vectors (catches failed bg indexing)
            mem.reindex_missing();
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("service state construction");
            let mem_supervisor = Arc::new(crate::mem::supervisor::Supervisor::new(
                crate::mem::supervisor::Supervisor::resolve_cli_path(&crate::paths::data_dir()),
                mem.db_path().to_path_buf(),
            ));

            let input_parser = Arc::new(
                crate::service::input_parser::InputParser::new(agent_registry.clone()),
            );
            let provider_registry = Arc::new(crate::provider::ProviderRegistry::new(
                agent_registry.clone(),
                chatbackend_registry.clone(),
            ));
            crate::service::plan::search_index::init_search_port(Arc::new(
                crate::service::plan::search_port_impl::MemSearchPort,
            ));
            let plan_store = Arc::new(crate::service::plan::PlanStore::new());
            let plan_turn_counter =
                Arc::new(crate::service::plan::reminders::TurnCounter::new());
            plan_store.bind_turn_counter(plan_turn_counter.clone());
            let live_pending_delivery_digests =
                crate::service::pending_delivery_payloads::live_payload_digests(
                    &states.mux.read().snapshot(),
                );
            let pending_delivery_payloads = Arc::new(
                crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
                    &crate::paths::data_dir(),
                    &live_pending_delivery_digests,
                )
                .expect("pending delivery payload store init"),
            );

            let user_last_activity = crate::user_last_activity::UserLastActivity::new();
            states.mesh.lock().build_hash = codeterm_build_info::hash().to_string();
            states.mesh.lock().last_activity = Arc::new(
                crate::mesh::UserLastActivityProvider(user_last_activity.clone()),
            );
            let workers = crate::mem::workers::GraphWorkerRegistry::load(
                crate::paths::data_dir().join("mem").join("workers.json"),
            );
            let remote_workers = crate::service::remote_workers::RemoteWorkerRegistry::load(
                crate::paths::data_dir().join("remote_workers.json"),
            );
            let delayed_send_path = crate::paths::data_dir().join("delayed_send.json");
            let delayed_send =
                crate::delayed_send::DelayedSendQueue::open(delayed_send_path.clone())
                    .unwrap_or_else(|error| {
                        log::error!(
                            "[delayed_send] startup read failed for {}: {}; continuing with an empty in-memory queue",
                            delayed_send_path.display(),
                            error
                        );
                        crate::delayed_send::DelayedSendQueue::empty(delayed_send_path)
                    });
            let startup_config = states.config.lock().clone();
            let kpi = crate::agent::kpi::KpiRecorder::from_config(
                &startup_config,
                &crate::paths::data_dir(),
            );
            {
                let handle = app_handle.clone();
                kpi.on_commit(move |plan_ids| {
                    codeterm_events::emit_everywhere(
                        &handle,
                        "kpi-recorded",
                        serde_json::json!({ "plan_ids": plan_ids }),
                    );
                });
            }
            let secrets =
                crate::secrets::new_state(&startup_config.integrations.secrets, &secret_registry);
            let services_store = crate::service::store::ServicesStore::open(
                &crate::paths::data_dir().join("services.db"),
            )
            .expect("services.db init");
            let usage_store = crate::usage::UsageStore::open(&crate::paths::data_dir())
                .expect("usage.db init");

            let mut extensions = crate::service::extensions::ExtensionsBuilder::default();
            extensions.insert(crate::service::workspace_cwd::WorkspaceRelocationLock(Arc::new(parking_lot::Mutex::new(()))));
            extensions.insert(crate::service::account::AccountCell(Arc::new(tokio::sync::OnceCell::new())));
            extensions.insert(crate::service::billing::BillingCell(Arc::new(tokio::sync::OnceCell::new())));
            extensions.insert(crate::service::analytics::AnalyticsCell(Arc::new(tokio::sync::OnceCell::new())));
            extensions.insert(crate::service::task_files::LaunchTaskFiles(Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()))));
            crate::ports::orchestration::register_extensions(&mut extensions);
            extensions.insert(crate::ports::orchestration::InstalledOrchestration::default());
            extensions.insert::<crate::ShellCapabilityState>(Arc::new(parking_lot::Mutex::new(Default::default())));
            extensions.insert::<Arc<crate::service::protocol_recorder::ProtocolRecorder>>(Arc::new(crate::service::protocol_recorder::ProtocolRecorder::new(
                    crate::paths::data_dir().join("logs/protocol"),
                    crate::paths::is_dev_instance(),
                )));
            extensions.insert::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>(Arc::new(Default::default()));
            extensions.insert::<Arc<crate::mem::supervisor::Supervisor>>(mem_supervisor);
            extensions.insert::<Arc<crate::plugin::activity::PluginActivityRegistry>>(Arc::new(crate::plugin::activity::PluginActivityRegistry::new()));
            extensions.insert::<Arc<crate::mem::worker_pool::WorkerPool>>(Arc::new(crate::mem::worker_pool::WorkerPool::new(
                    crate::config::memory_config().workers_per_provider,
                )));
            extensions.insert::<Arc<crate::service::process_tree::ProviderTreeWitnesses>>(Arc::new(Default::default()));
            extensions.insert(crate::service::BridgeOutboundRx(Arc::new(Mutex::new(Some(bridge_outbound_rx)))));
            extensions.insert::<Arc<crate::bridge::supervisor::BridgeSupervisor>>(Arc::new(bridge::supervisor::BridgeSupervisor::new()));
            extensions.insert::<Arc<codeterm_bridge::bridge::threads::ThreadService>>(Arc::new(codeterm_bridge::bridge::threads::ThreadService::new(
                    codeterm_bridge::bridge::threads::ThreadRegistry::load(
                        crate::service::bridge_threads::registry_path(),
                    ),
                )));
            extensions.insert::<crate::service::agent_restart::RestartTracker>(crate::service::agent_restart::new_restart_tracker());
            extensions.insert::<crate::delayed_send::DelayedSendQueue>(delayed_send);
            extensions.insert::<Arc<crate::secrets::SecretRegistry>>(secret_registry.clone());
            extensions.insert::<Arc<crate::transcribe::TranscribeRegistry>>(transcribe_registry.clone());
            extensions.insert::<Arc<codeterm_plugin::converter::ConverterRegistry>>(converter_registry.clone());
            extensions.insert::<Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>(graph_node_registry.clone());
            extensions.insert::<Arc<codeterm_plugin::event_source::EventSourceRegistry>>(event_source_registry.clone());
            extensions.insert::<codeterm_plugin::graph_events::SubscriptionTable>(graph_event_subscriptions.clone());
            extensions.insert::<crate::glance::GlanceRegistryHandle>(glance_registry.clone());
            extensions.insert::<crate::bubble::BubbleRegistryHandle>(bubble_registry.clone());
            extensions.insert::<crate::repowatch::RepoWatchRegistryHandle>(repo_watch);
            extensions.insert::<crate::view_host::ViewRegistryHandle>(view_registry.clone());
            extensions.insert::<Arc<crate::service::tab_transport_chat::TabReopenGuards>>(Arc::new(Default::default()));
            extensions.insert::<crate::chatpreprocessor::ChatPreprocessorRegistryHandle>(chat_preprocessor_registry.clone());
            extensions.insert::<crate::agent_commands::AgentCommandRegistryHandle>(agent_command_registry.clone());
            extensions.insert::<codeterm_plugin::WebhookRegistryHandle>(webhook_registry.clone());
            extensions.insert::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>(cap_registries.local_recording.clone());
            extensions.insert::<Arc<crate::service::local_recording::RecordingSelectionStore>>(Arc::new(crate::service::local_recording::RecordingSelectionStore::open(crate::paths::data_dir().join("local-recording-selections.json"))));
            extensions.insert(crate::service::receiver_target::LocalRecordingTargets(Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(crate::paths::data_dir().join("local-recording-targets.json")))));
            extensions.insert::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>(tab_menu_registry.clone());
            extensions.insert::<codeterm_plugin::surfaces::SurfaceRegistryHandle>(surface_registry);
            extensions.insert::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>(crate::service::surface::builtin_runtimes());
            extensions.insert::<Arc<crate::service::surface::SurfaceActivityGate>>(Default::default());
            extensions.insert(crate::service::receiver_target::ReceiverTargets(Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(
                    crate::service::receiver_target::ReceiverTargetStore::default_path(),
                ))));
            extensions.insert(crate::service::MuxEmitPending(Arc::new(std::sync::atomic::AtomicBool::new(false))));
            extensions.insert::<Arc<crate::service::MuxEmitStats>>(Arc::new(crate::service::MuxEmitStats::default()));
            extensions.insert(crate::service::PublishedTabStatuses(Arc::new(Default::default())));
            extensions.insert(crate::service::output_stream_activity::OutputStreamActivityTx(crate::service::output_stream_activity::channel()));
            extensions.insert::<crate::pasteboard::PasteboardState>(crate::pasteboard::PasteboardState::new());
            extensions.insert::<Arc<crate::service::input_parser::InputParser>>(input_parser);
            extensions.insert::<crate::usage::TabActivity>(crate::usage::TabActivity::new());
            extensions.insert::<Arc<crate::user_last_activity::UserLastActivity>>(user_last_activity);
            extensions.insert(crate::usage::poller::UsagePollerSlot(Arc::new(parking_lot::Mutex::new(None))));
            extensions.insert(crate::service::PlanWatchers(Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()))));
            extensions.insert(crate::service::LoadedPlanProjects(Arc::new(parking_lot::Mutex::new(
                    std::collections::HashSet::new(),
                ))));
            extensions.insert(crate::service::PlanStartupLoaded(Arc::new(std::sync::atomic::AtomicBool::new(false))));
            extensions.insert::<Arc<crate::service::hierarchy::stalled_team::StallEpisodeTracker>>(Arc::new(Default::default()));
            extensions.insert::<Arc<crate::service::hierarchy::heartbeat_gate::Gate>>(Arc::new(Default::default()));
            extensions.insert(crate::service::WorkerIdentityAllocator(Arc::new(parking_lot::Mutex::new(
                    crate::service::WorkerPersonaReservations::default(),
                ))));
            let svc_ctx = ServiceContext {
                lifecycle_journal: crate::session::lifecycle::TabLifecycleJournal::shared_arc(),
                #[cfg(any(test, feature = "test-support"))]
                ready_user_send_tasks: Arc::new(service::test_fixture::TestReadySendTasks::default()),
                agent_pad_data_dir: crate::paths::data_dir(),
                mux: states.mux.clone(),
                pty: states.pty.clone(),
                output_buf: states.output_buffer.clone(),
                screen_state: states.screen_state.clone(),
                notif: states.notif.clone(),
                events: states.event_broadcaster.clone(),
                app_handle: app_handle.clone(),
                agent_tracker: states.agent_tracker.clone(),
                report_store: states.report_store.clone(),
                kpi,
                plan_store,
                plan_turn_counter,
                pending_delivery_payloads,
                chat_send_ledger: Arc::new(
                    crate::service::chat_send_transaction::ChatSendLedger::default(),
                ),
                deferred_session_bindings: Arc::new(Default::default()),
                agent_registry,
                mem,
                activity: crate::mem::activity::ActivityRing::new(500),
                workers,
                prompt_detector,
                pty_ownership: Default::default(),
                agent_run_pty_map: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
                remote_workers: Arc::new(remote_workers),
                bridge_outbound: bridge_outbound_tx,
                chat_registry: crate::service::tab_chat::ChatRegistry::new(),
                secrets,
                supporter_models: decision_model_registry,
                chatbackend_registry: chatbackend_registry.clone(),
                provider_registry,
                local_recordings: Arc::new(crate::service::local_recording::RecordingLeases::default()),
                services_store: Arc::new(services_store),
                tokens: Arc::new(crate::service::tokens::TokenStore::new()),
                usage_store,
                worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
                gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
                extensions: Arc::new(extensions.finish()),
            };
            crate::service::supporter::runtime_with_notifications(&svc_ctx);
            let service_state: ServiceState = Arc::new(svc_ctx);
            crate::ports::orchestration::install(&service_state);
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
                crate::service::plan::task_key_port_impl::MuxTaskKeyPort::new(
                    service_state.mux.clone(),
                    service_state.app_handle.clone(),
                ),
            ));
            service_state
                .mem
                .extraction_authority()
                .set_cancel_sink(Arc::new(crate::mem_host::WorkerCancelSink(
                    service_state.clone(),
                )));
            {
                let config = states.config.lock().clone();
                if config.general_agent.enabled && config.general_agent.first_run_complete {
                    let default_chat_view =
                        crate::service::general_agent::default_chat_view_for_provider(
                            &config,
                            &service_state.agent_registry,
                            &config.general_agent.agent_provider,
                        );
                    crate::service::general_agent::apply_general_chat_view_default(
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
                crate::service::plugin_prompt::refresh(service_state.ext::<crate::agent_commands::AgentCommandRegistryHandle>());
                crate::service::plugin_skills::spawn_sync(service_state.mem.clone(), service_state.ext::<crate::agent_commands::AgentCommandRegistryHandle>().clone());
                for provider in chatbackend_registry.active() {
                    crate::agent::model_binary_watch::install_chatbackend(
                        provider,
                        chatbackend_registry.clone(),
                    );
                }
                let _ = chatbackend_registry.start_discovery();
            }
            crate::service::agent_runtime::restore::all(&service_state);
            app.manage(service_state.clone());
            crate::service::account::register_deep_links(&app_handle, service_state.clone())?;
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
            crate::delayed_send::start_scheduler(service_state.clone());
            crate::service::start_plan_reminder_scheduler(service_state.clone());
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("restored plan loading");
            startup::restore_plans(&service_state);
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("background service initialization");
            {
                let store = service_state.services_store.clone();
                let activity = crate::service::activity::ActivityWriter::new(store.conn());
                crate::service::health::scheduler::start(store, activity, app.handle().clone());
            }

            // Secrets init happens before API listener binds to avoid auth calls hitting uninitialized state.

            // Librarian: hourly file-sync + skill-folder scan, run as a subprocess via the supervisor so a panic / OOM cannot take the daemon down. First fire after a 30s warm-up.
            {
                let supervisor = service_state.ext::<Arc<crate::mem::supervisor::Supervisor>>().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    loop {
                        match supervisor.run(crate::mem::supervisor::WorkerKind::LibrarianOnce).await {
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
            if crate::config::memory_config().auto_extract {
                let svc_for_extract = service_state.clone();
                let queued = crate::mem_host::resume_extract_pending(
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
                let log_store = crate::logs::LogStore::open(&crate::paths::data_dir())
                    .expect("app_logs.db init");
                log_store.attach_app(app_handle.clone());
                crate::logs::set_global(log_store.clone());
                log_store.start_flush_loop();
            }

            {
                let poller = crate::usage::UsagePoller::new(
                    (*service_state).clone(),
                    service_state.usage_store.clone(),
                    service_state.ext::<crate::usage::TabActivity>().clone(),
                );
                *service_state.ext::<crate::usage::poller::UsagePollerSlot>().0.lock() = Some(poller.clone());
                poller.spawn();
            }
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("platform and server initialization");
            startup::setup_conpty_host(app);
            startup::install_bundled_cli(app);
            drop(startup::restore_complete(&service_state));
            startup::start_servers(app, &states, &service_state);
            startup::frontend_shell::start(app_handle.clone(), states.rest_api.clone());
            service_state.schedule_restored_agent_delivery_reconciliation();
            crate::service::provider_hooks::ensure_integrations_for_all(
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
            service::provider_update::check_available(
                service_state.clone(),
                api_port,
                allow_non_prod_updates,
            );
            if states.config.lock().api.enabled {
                crate::liveness::start_api_liveness_watchdog(
                    app_handle.clone(),
                    states.mesh.clone(),
                    states.rest_api.clone(),
                    states.config.lock().api.port,
                );
            }
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Err(error) = tokio::task::spawn_blocking(crate::crash::lifecycle::heartbeat).await {
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
                    let new_reports = crate::crash::check_previous_crashes();
                    if !new_reports.is_empty() {
                        let dir = crate::crash::report::crashes_dir();
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
                        codeterm_events::emit_everywhere(
                            &app_handle,
                            "crash_report_available",
                            serde_json::json!({
                                "dir": dir.to_string_lossy(),
                                "files": files,
                                "recovery": crate::crash::lifecycle::recovery_state(),
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
            service::sync_watcher::start(crate::paths::data_dir(), service_state.agent_registry.clone());
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
        .build(startup::frontend_shell::application_context(context))
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

#[cfg(test)]
#[path = "../../../vendor/tauri-runtime-wry/src/owner_thread.rs"]
mod windows_rc_ownership_regression;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(any(test, feature = "test-support"))]
pub mod persistence_test_support;
