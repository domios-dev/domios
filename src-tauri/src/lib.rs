pub mod agent;
pub mod agent_commands;
mod api;
pub mod attachments;
mod autostart;
pub mod bridge;
pub mod bubble;
pub mod chatbackend;
pub mod chatpipeline;
pub mod chatpreprocessor;
mod clipboard_files;
mod commands;
mod commands_mesh;
mod config;
mod crash;
pub mod cron;
pub mod delayed_send;
pub mod dispatch;
pub mod glance;
mod keepawake;
mod liveness;
pub mod logs;
pub mod mem;
pub mod mesh;
pub mod message_markers;
pub mod multiplexer;
mod notification;
pub mod pasteboard;
pub mod paths;
pub mod plugin;
pub mod process_util;
pub mod prompts;
pub mod provider;
pub mod pty;
pub mod resize_owner;
pub mod secrets;
pub mod service;
pub mod session;
pub mod shell;
mod socket;
mod startup;
pub mod transcribe;
pub mod update;
pub mod usage;
pub mod view;
pub mod view_host;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use multiplexer::commands::MuxState;
use notification::NotifState;
use pty::manager::{OutputBuffer, PtyManager};
use pty::screen_state::ScreenState;
use service::{ServiceContext, ServiceState};
use session::SessionState;

pub type PtyState = Arc<Mutex<PtyManager>>;
pub type OutputBufferState = Arc<Mutex<OutputBuffer>>;
pub type ScreenStateHandle = Arc<Mutex<ScreenState>>;
pub type AgentTrackerState = Arc<Mutex<agent::status::PaneStatusTracker>>;
pub type ReportStoreState = Arc<Mutex<agent::reports::ReportStore>>;
pub type PromptDetectorState = Arc<Mutex<agent::prompt::PromptDetector>>;
pub use cron::scheduler::{CronState, CronWake};
pub type UpdateStateHandle = Arc<Mutex<update::UpdateState>>;
pub type MeshStateHandle = mesh::MeshState;
pub type KeepAwakeStateHandle = keepawake::KeepAwakeState;
pub type ResizeOwnerState = resize_owner::ResizeOwnerState;

pub use startup::{spawn_telegram_bridge, start_telegram_bridge};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init, otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    // Install panic + native crash handlers BEFORE any thread spawn or logging init. Reports land in ~/.codeterm/crashes/ regardless of where the
    // crash originates (tokio worker, Tauri main, FFI thread, signal handler).
    crash::install();

    startup::setup_logging();

    log::info!("CodeTerm starting...");

    // Write shell integration scripts (OSC 7 CWD tracking)
    pty::shell_integration::ensure_scripts();

    // Initialize all shared states
    let states = startup::init_states();

    // Seed canonical ~/.codeterm/skills/, then mirror into provider dirs agents load from (#178).
    commands::seed_bundled_skills();
    commands::mirror_skills_to_providers();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
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

    // Single-instance: only in release builds so dev and prod can run side by side
    if !cfg!(debug_assertions) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }

    builder
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
        // ServiceState is added in .setup() after AppHandle is available
        .invoke_handler(tauri::generate_handler![
            // PTY commands
            commands::get_windows_build_number,
            commands::get_default_cwd,
            commands::spawn_pty,
            commands::read_pty,
            commands::write_pty,
            commands::write_pty_string,
            commands::get_pane_output,
            commands::get_pane_history_raw,
            commands::get_pane_text,
            commands::list_agent_providers,
            commands::wait_shell_ready,
            commands::read_session_jsonl,
            commands::resize_pty,
            commands::claim_resize_owner,
            commands::kill_pty,
            commands::suggest_install_pwsh,
            commands::get_shell_diagnostics,
            commands::restart_app,
            commands::restart_general_agent,
            commands::get_network_info,
            commands::get_serve_status,
            commands::enable_web_access,
            commands::disable_web_access,
            commands::generate_api_token,
            commands::report_webgl_status,
            commands::save_clipboard_image,
            commands::save_clipboard_file,
            commands::materialize_dropped_files,
            clipboard_files::read_clipboard_files,
            clipboard_files::read_clipboard_text,
            commands::save_temp_file,
            commands::copy_image_to_clipboard,
            commands::check_for_update,
            commands::install_app_update,
            commands::open_folder,
            commands::open_url,
            commands::test_notification,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
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
            multiplexer::commands::mux_set_active_tab_and_workspace,
            multiplexer::commands::mux_rename_tab,
            multiplexer::commands::mux_close_pane,
            multiplexer::commands::mux_reopen_closed_pane,
            multiplexer::commands::mux_set_focused_pane,
            multiplexer::commands::mux_link_pty,
            multiplexer::commands::mux_rename_pane,
            multiplexer::commands::mux_update_pane_cwd,
            multiplexer::commands::mux_set_pane_agent_type,
            multiplexer::commands::mux_unlock_pane_agent_type,
            multiplexer::commands::mux_set_chat_view,
            multiplexer::commands::detect_claude_session_id,
            multiplexer::commands::detect_pane_session_id,
            multiplexer::commands::mux_set_claude_session_id,
            commands::chat_start,
            commands::session_transcript,
            commands::chat_stop,
            commands::chat_backfill,
            commands::chat_send_text,
            commands::open_chat_backend_pane,
            commands::chatbackend_session_info,
            commands::chatbackend_switch_model,
            commands::transcribe_voice_attachment,
            commands::delayed_send_list,
            commands::delayed_send_create,
            commands::delayed_send_update,
            commands::delayed_send_delete,
            commands::delayed_send_send_now,
            commands::check_cli_installed,
            commands::install_provider_in_pane,
            commands::plugin_manager_snapshot,
            commands::plugin_manager_action,
            commands::plugin_glance,
            commands::plugin_bubble,
            commands::plugin_view_call,
            multiplexer::commands::build_agent_launch_command,
            multiplexer::commands::mux_spawn_pane,
            multiplexer::commands::mux_set_pane_role,
            multiplexer::commands::mux_clear_pane_role,
            multiplexer::commands::mux_set_can_spawn,
            multiplexer::commands::mux_respond_prompt,
            multiplexer::commands::mux_batch,
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
            // Provider integration (hooks/callbacks) — generic dispatchers
            commands::check_provider_integration,
            commands::install_provider_integration,
            commands::uninstall_provider_integration,
            commands::check_provider_rule,
            commands::install_provider_rule,
            // Session commands
            session::save_session,
            session::save_session_named,
            session::list_saved_sessions,
            session::load_session,
            // Cron commands
            commands::cron_list,
            commands::cron_get,
            commands::cron_create,
            commands::cron_update,
            commands::cron_delete,
            commands::cron_toggle,
            commands::cron_trigger,
            commands::cron_history,
            commands::cron_read_log,
            // Services commands
            commands::services_list,
            commands::services_create,
            commands::services_get,
            commands::services_update,
            commands::services_delete,
            commands::services_regenerate_token,
            commands::activity_list,
            commands::services_smart_add_inspect,
            // Mesh commands
            commands_mesh::mesh_get_status,
            commands_mesh::mesh_list_peers,
            commands_mesh::mesh_refresh_discovery,
            commands_mesh::mesh_connect,
            commands_mesh::mesh_approve_pairing,
            commands_mesh::mesh_reject_pairing,
            commands_mesh::mesh_disconnect,
            commands_mesh::mesh_exit,
            commands_mesh::mesh_add_manual,
            commands_mesh::mesh_start_discovery,
            commands_mesh::mesh_start_broadcast,
            commands_mesh::mesh_stop_broadcast,
            commands_mesh::mesh_scan_peers,
            commands_mesh::mesh_set_access,
            commands_mesh::mesh_peer_services_list,
            // Keep-awake commands
            keepawake::keepawake_start,
            keepawake::keepawake_stop,
            keepawake::keepawake_status,
            // Usage tracker commands
            commands::usage_latest,
            commands::usage_history,
            commands::usage_daily_rollup,
            commands::usage_projection,
            commands::usage_refresh,
        ])
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                startup::setup_window_styling(&window);
            }

            // Create unified service context (available to both API and Tauri commands)
            let app_handle = app.handle().clone();

            // Resume any keep-awake session that was active when CodeTerm last shut down. Indefinite → resume; timed + remaining → resume with the
            // remainder; timed + already expired → drop the snapshot.
            keepawake::reap_orphaned_children_on_startup();
            keepawake::resume_from_disk(&states.keepawake, &app_handle);

            // Bootstrap plugin system — agent-claude is a bundled native plugin
            agent::model_catalog::ensure_readme_installed();
            // One-time: carry a pre-existing native Bitwarden setup (server URL +
            // saved session/master) into the Bitwarden plugin's namespace, then
            // persist. Native Bitwarden is retired; the plugin inherits the user's
            // existing login so they don't have to re-unlock.
            {
                let mut cfg = states.config.lock();
                if crate::secrets::migrate_native_bitwarden_to_plugin(&mut cfg) {
                    let snapshot = cfg.clone();
                    drop(cfg);
                    let _ = crate::config::save_config_pub(&snapshot);
                }
            }

            let agent_registry = Arc::new(agent::registry::AgentRegistry::new());
            // Capability registries — populated below as JS plugins load, then
            // handed to ServiceContext. Transcribe is seeded with the built-in
            // HTTP engine so voice notes work with no plugin (zero-regression).
            let secret_registry = Arc::new(crate::secrets::SecretRegistry::new());
            let transcribe_registry = Arc::new(crate::transcribe::TranscribeRegistry::new());
            let glance_registry: crate::glance::GlanceRegistryHandle =
                Arc::new(crate::glance::GlanceRegistry::new());
            let bubble_registry: crate::bubble::BubbleRegistryHandle =
                Arc::new(crate::bubble::BubbleRegistry::new());
            let view_registry: crate::view_host::ViewRegistryHandle =
                Arc::new(crate::view_host::ViewRegistry::new());
            let chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle =
                Arc::new(crate::chatbackend::ChatBackendRegistry::new());
            let chat_preprocessor_registry: crate::chatpreprocessor::ChatPreprocessorRegistryHandle =
                Arc::new(crate::chatpreprocessor::ChatPreprocessorRegistry::new());
            let agent_command_registry: crate::agent_commands::AgentCommandRegistryHandle =
                Arc::new(crate::agent_commands::AgentCommandRegistry::new());
            // One bundle of capability registry handles, shared by the startup
            // registration loop AND the hot-reload watcher so both go through the
            // same `register_plugin` path (§1.8 P1 — no drift between them).
            let cap_registries = plugin::js_runtime::CapabilityRegistries {
                secret: secret_registry.clone(),
                transcribe: transcribe_registry.clone(),
                glance: glance_registry.clone(),
                bubble: bubble_registry.clone(),
                view: view_registry.clone(),
                chatbackend: chatbackend_registry.clone(),
                chat_preprocessor: chat_preprocessor_registry.clone(),
                agent_command: agent_command_registry.clone(),
            };
            // Plugin VMs collected as they load, so the host `ServiceContext`
            // can be late-bound into each sandbox once it is built below (the
            // VMs exist before `ServiceContext` does — plan §1.5 F4 / C0).
            let mut plugin_vms: Vec<Arc<plugin::js_runtime::PluginVm>> = Vec::new();
            // No native transcriber: the bundled `transcriber` plugin registers
            // itself below (everything-as-plugins). The registry starts empty.
            let mut plugin_manager = plugin::PluginManager::new();
            plugin_manager.set_agent_registry(agent_registry.clone());
            plugin_manager.set_event_broadcaster(states.event_broadcaster.clone());

            // Bridge ContributionsChanged → frontend event + mem bridge rule refresh.
            {
                use tauri::Emitter;
                let mut sub = states.event_broadcaster.subscribe();
                let bridge_handle = app_handle.clone();
                let bridge_registry = agent_registry.clone();
                tauri::async_runtime::spawn(async move {
                    while let Ok(ev) = sub.recv().await {
                        if let crate::api::events::SystemEvent::ContributionsChanged { kind } = &ev {
                            let _ = bridge_handle.emit(
                                "contributions-changed",
                                serde_json::json!({ "kind": kind }),
                            );
                            if kind == "agent_provider" {
                                crate::service::sync_watcher::install_bridge_rules_for_all(
                                    &bridge_registry,
                                );
                                crate::commands::mirror_skills_to_providers();
                            }
                        }
                    }
                });
            }

            // Register and activate bundled plugins
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

            // Wire the event bus to plugins: previously `dispatch_event` had no
            // production caller (dead code). Keep the manager alive in the
            // subscriber task and fan SystemEvents to active plugins' on_event.
            // Survives broadcast lag; exits only when the bus closes (shutdown).
            {
                let plugin_manager = std::sync::Arc::new(plugin_manager);
                let pm = plugin_manager.clone();
                let mut sub = states.event_broadcaster.subscribe();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match sub.recv().await {
                            Ok(ev) => pm.dispatch_event(&ev),
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
                use std::path::PathBuf;
                agent_registry.begin_startup_provider_loading(
                    crate::plugin::system_plugins::SYSTEM_PLUGIN_HASHES
                        .iter()
                        .map(|(id, _)| *id),
                );
                let workspace_root: Option<PathBuf> = Some(PathBuf::from(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/..")
                ));
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
                    // Hold a VM handle so the ServiceContext can be late-bound
                    // into its sandbox after service init (C0). `p` itself is
                    // moved into a registry further down.
                    plugin_vms.push(p.vm().clone());
                    log::info!(
                        "[js-plugin] {id} trust={:?} caps(agent={} secret={} transcribe={})",
                        trust,
                        caps.agent_provider,
                        caps.secret_backend,
                        caps.transcriber
                    );

                    // Capability adapters share the plugin's VM. A capability-only
                    // plugin (agent_provider=false) is NOT registered as an agent.
                    // Single seam shared with the hot-reload watcher (§1.8 P1).
                    cap_registries.register_plugin(p.manifest(), p.vm());
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
                        // JS takes authority; register_provider preserves the
                        // displaced native impl as a capability fallback the JS
                        // provider delegates to for methods it doesn't export.
                        p.set_registry(agent_registry.clone());
                        log::info!(
                            "[js-plugin] {id} registered as authoritative \
                            (set CODETERM_NATIVE_PROVIDERS={id} to rollback)"
                        );
                        agent_registry.register_provider(Box::new(p));
                        plugin::audit::append_daemon_reloaded(&id, &version);
                    }
                }
                agent_registry.finish_startup_provider_loading();

                // Hot-reload watcher: polls the same roots every 2s and (a) discovers new plugin folders dropped into ~/.codeterm/plugins/, (b) reloads
                // Hot-reload keeps plugin VMs in sync: reload existing plugins and drop vanished plugin dirs.
                let watcher = plugin::js_runtime::HotReloadRegistry::new(
                    roots.clone(),
                    agent_registry.clone(),
                    cap_registries.clone(),
                );
                // Seed tracked-state for already-registered plugins (and mark them
                // activated-at-startup) so the first tick doesn't re-register them
                // or mis-flag startup `requiresReload` plugins as pending (§1.8).
                watcher.seed();
                // Track each startup-loaded plugin's live VM so a later
                // config-only edit re-stamps settings into it instead of
                // rebuilding (which would wipe in-memory chatBackend sessions).
                for vm in &plugin_vms {
                    watcher.register_vm(vm.plugin_id(), vm);
                }
                watcher.start();
                log::info!(
                    "[js-plugin] hot-reload watcher started on {} roots",
                    roots.len()
                );
            }

            // All providers have called `model_catalog::register(...)` in their constructors by now — refresh the `<provider>.example.toml` files so users
            // always see the current build's defaults.
            agent::model_catalog::refresh_example_files();

            let prompt_detector = Arc::new(Mutex::new(agent::prompt::PromptDetector::new()));
            let (bridge_outbound_tx, bridge_outbound_rx) = bridge::manager::create_outbound_channel();

            let mem = mem::MemManager::open(&crate::paths::data_dir())
                .expect("mem manager init failed");
            match mem.seed_bundled_if_needed() {
                Ok(n) if n > 0 => log::info!("[mem] seeded {} bundled starter items", n),
                Ok(_) => log::info!("[mem] starter items already seeded"),
                Err(e) => log::warn!("[mem] seed failed: {}", e),
            }

            // Sync identity.md + soul.md into mem for search/vectorization
            crate::service::general_agent::sync_identity_to_mem(&mem);

            // Re-index any items missing vectors (catches failed bg indexing)
            mem.reindex_missing();

            // Auto-install each provider's CodeTerm integration (hooks / callbacks). Providers without an integration no-op via the AgentProvider trait
            // default. For Claude this installs the Notification / Stop / PreCompact / SessionStart hooks that POST events back into our HTTP API.
            commands::ensure_integrations_for_all(&agent_registry);

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
            let svc_ctx = ServiceContext {
                mux: states.mux.clone(),
                pty: states.pty.clone(),
                output_buf: states.output_buffer.clone(),
                screen_state: states.screen_state.clone(),
                notif: states.notif.clone(),
                events: states.event_broadcaster.clone(),
                app_handle: app_handle.clone(),
                agent_tracker: states.agent_tracker.clone(),
                report_store: states.report_store.clone(),
                agent_registry,
                mem,
                mem_supervisor,
                activity: crate::mem::activity::ActivityRing::new(500),
                workers: crate::mem::workers::GraphWorkerRegistry::load(
                    crate::paths::data_dir().join("mem").join("workers.json"),
                ),
                plugin_activity: Arc::new(crate::plugin::activity::PluginActivityRegistry::new()),
                worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(
                    crate::config::memory_config().workers_per_provider,
                )),
                prompt_detector,
                agent_run_pty_map: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
                bridge_outbound: bridge_outbound_tx,
                bridge_outbound_rx: Arc::new(Mutex::new(Some(bridge_outbound_rx))),
                bridge_supervisor: Arc::new(bridge::supervisor::BridgeSupervisor::new()),
                restart_tracker: crate::service::agent_restart::new_restart_tracker(),
                chat_registry: crate::service::pane_chat::ChatRegistry::new(),
                delayed_send: crate::delayed_send::DelayedSendQueue::open(
                    crate::paths::data_dir().join("delayed_send.json"),
                )
                .expect("delayed_send init"),
                secret_registry: secret_registry.clone(),
                secrets: {
                    let cfg_guard = states.config.lock();
                    let cfg = cfg_guard.integrations.secrets.clone();
                    drop(cfg_guard);
                    crate::secrets::new_state(&cfg, &secret_registry)
                },
                transcribe_registry: transcribe_registry.clone(),
                glance_registry: glance_registry.clone(),
                bubble_registry: bubble_registry.clone(),
                view_registry: view_registry.clone(),
                chatbackend_registry: chatbackend_registry.clone(),
                provider_registry,
                chat_preprocessor_registry: chat_preprocessor_registry.clone(),
                agent_command_registry: agent_command_registry.clone(),
                mux_emit_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                services_store: Arc::new(
                    crate::service::store::ServicesStore::open(
                        &crate::paths::data_dir().join("services.db"),
                    )
                    .expect("services.db init"),
                ),
                tokens: Arc::new(crate::service::tokens::TokenStore::new()),
                pasteboard: crate::pasteboard::PasteboardState::new(),
                input_parser,
                usage_store: crate::usage::UsageStore::open(&crate::paths::data_dir())
                    .expect("usage.db init"),
                usage_activity: crate::usage::PaneActivity::new(),
                usage_poller: Arc::new(parking_lot::Mutex::new(None)),
            };
            let service_state: ServiceState = Arc::new(svc_ctx);
            // Late-bind the ServiceContext into every loaded plugin's sandbox
            // (C0). Weak so plugin VMs never keep the service alive past teardown.
            for vm in &plugin_vms {
                vm.set_service(Arc::downgrade(&service_state));
            }
            // Same late-bind for the hot-reload watcher (P4): plugins installed or
            // reloaded AFTER startup get the service threaded into their VMs too,
            // so host.mem/worker/agent work without a daemon restart.
            plugin::js_runtime::set_watcher_service(Arc::downgrade(&service_state));
            app.manage(service_state.clone());
            crate::delayed_send::start_scheduler(service_state.clone());

            // Re-kick session-name + model pollers for panes restored from snapshot: set_pane_agent_type fires on transitions, but a restore brings
            // agent_type back already-set so no transition ever happens. Walk the restored panes once at boot.
            service_state.kick_off_pollers_for_existing_agents();

            {
                let store = service_state.services_store.clone();
                let activity = crate::service::activity::ActivityWriter::new(store.conn());
                crate::service::health::scheduler::start(store, activity, app.handle().clone());
            }

            // Secrets init happens before API listener binds to avoid auth calls hitting uninitialized state.

            // Librarian: hourly file-sync + skill-folder scan, run as a subprocess via the supervisor so a panic / OOM cannot take the daemon down. First fire after a 30s warm-up.
            {
                let supervisor = service_state.mem_supervisor.clone();
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

            // Graph layout: build the projection once at startup if needed, then
            // tick a debounced cache flush. The heavy projection runs on a
            // blocking thread so it never stalls the async runtime.
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

            // Resume extraction for items that were saved but never finished (previous run crashed mid-extraction, LLM API errored, bulk import).
            // run_extract_pending returns fast after spawning its tokio task.
            {
                let svc_for_extract = service_state.clone();
                let queued = crate::mem::auto_extract::run_extract_pending(
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
                    service_state.usage_activity.clone(),
                );
                *service_state.usage_poller.lock() = Some(poller.clone());
                poller.spawn();
            }

            startup::install_bundled_cli(app);
            startup::start_servers(app, &states, &service_state);
            if states.config.lock().api.enabled {
                crate::liveness::start_api_liveness_watchdog(
                    app_handle.clone(),
                    states.mesh.clone(),
                    states.config.lock().api.port,
                );
            }
            startup::build_system_tray(app)?;

            // After webview is ready, check for crash reports from a previous session and emit an event the frontend can show as a toast. Delay a tick so
            // the React side has time to attach the listener.
            {
                use tauri::Emitter;
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
                        let _ = app_handle.emit(
                            "crash_report_available",
                            serde_json::json!({
                                "dir": dir.to_string_lossy(),
                                "files": files,
                            }),
                        );
                    }
                });
            }

            service::sync_watcher::start(crate::paths::data_dir(), service_state.agent_registry.clone());
            startup::start_background_tasks(&app_handle, &states);

            // PTY read loops are started by spawn_pty command (initiated by frontend)
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Only handle main window, not notification popups
                if window.label() != "main" { return; }

                // Save session
                let app = window.app_handle();
                if let (Some(mux), Some(session)) = (
                    app.try_state::<MuxState>(),
                    app.try_state::<SessionState>(),
                ) {
                    let snapshot = mux.write().to_session_snapshot();
                    if let Err(e) = session.lock().save(&snapshot) {
                        log::error!("Failed to save session: {}", e);
                    } else {
                        log::info!("Session saved (close-to-tray)");
                    }
                }

                // Hide to tray instead of quitting
                api.prevent_close();
                let _ = window.hide();
                log::info!("Window hidden to tray");
            }
        })
        .build(tauri::generate_context!())
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

            // On any clean process exit (tray-Quit, Cmd+Q after window close, SIGTERM caught by Tauri), checkpoint+truncate the mem WAL so a stale ~4MB
            // log doesn't survive into the next run. The updater's graceful_shutdown already does this on the in-place upgrade path; this catches every other shutdown route.
            if let tauri::RunEvent::Exit = _event {
                keepawake::cleanup_active_children_for_exit();
                if let Some(svc) = _app.try_state::<service::ServiceState>() {
                    match svc.mem.checkpoint_and_close() {
                        Ok((busy, log_frames, checkpointed)) => log::info!(
                            "[shutdown] mem WAL checkpointed busy={} log_frames={} checkpointed={}",
                            busy, log_frames, checkpointed
                        ),
                        Err(e) => log::warn!("[shutdown] mem WAL checkpoint failed: {}", e),
                    }
                }
            }
        });
}
