pub mod agent;
pub mod agent_commands;
mod api;
pub mod attachments;
mod autostart;
pub mod bridge;
pub mod bridge_host;
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
mod fdlimit;
pub mod glance;
mod keepawake;
mod liveness;
pub mod lock_hold;
pub mod logs;
pub mod mem;
pub mod mem_host;
pub mod mesh;
pub mod message_markers;
pub mod multiplexer;
#[cfg(windows)]
mod native_resize;
mod notification;
pub mod pasteboard;
pub mod paths;
pub mod plan;
pub mod plugin;
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
mod socket;
mod startup;
pub mod transcribe;
pub mod transcribe_adapter;
pub mod update;
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
use notification::NotifState;
use pty::manager::{OutputBuffer, PtyManager};
use pty::screen_state::ScreenState;
use service::{ServiceContext, ServiceState};
use session::SessionState;

pub type PtyState = Arc<lock_hold::TimedMutex<PtyManager>>;
pub type OutputBufferState = Arc<lock_hold::TimedMutex<OutputBuffer>>;
pub type ScreenStateHandle = Arc<lock_hold::TimedMutex<ScreenState>>;
pub type AgentTrackerState = Arc<lock_hold::TimedMutex<agent::status::TabStatusTracker>>;
pub type ShellCapabilityState = Arc<Mutex<agent::command_detection::ShellCapabilityTracker>>;
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
    // Never automatic: only an explicit `repair-user-path` argv reaches it.
    startup::run_user_path_repair_if_invoked();

    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init, otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    // crash originates (tokio worker, Tauri main, FFI thread, signal handler).
    crash::install();

    startup::setup_logging();

    log::info!("CodeTerm starting...");
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
        .manage(states.rest_api.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_windows_build_number,
            commands::get_default_cwd,
            commands::spawn_pty,
            commands::read_pty,
            commands::write_pty,
            commands::write_pty_string,
            commands::interrupt_agent,
            commands::write_pty_launch_command,
            commands::get_tab_output,
            commands::get_tab_history_raw,
            commands::get_tab_text,
            commands::session_binding_state,
            commands::shell_command_snapshot,
            commands::list_agent_providers,
            commands::get_remote_workers,
            commands::wait_shell_ready,
            commands::read_session_jsonl,
            commands::resize_pty,
            commands::claim_resize_owner,
            commands::kill_pty,
            commands::suggest_install_pwsh,
            commands::get_shell_diagnostics,
            commands::frontend_ready,
            commands::frontend_readiness_generation,
            commands::restart_app,
            commands::restart_rest_api,
            commands::get_rest_api_status,
            commands::rest_api_port_released,
            commands::restart_general_agent,
            commands::restart_general_agent_tab,
            commands::restart_agent_tab,
            commands::list_provider_update_prompts,
            commands::respond_provider_update_prompt,
            commands::acknowledge_provider_update_prompt,
            commands::complete_general_agent_first_run,
            commands::get_network_info,
            commands::get_serve_status,
            commands::enable_web_access,
            commands::disable_web_access,
            commands::generate_api_token,
            commands::report_webgl_status,
            commands::list_plan_index,
            commands::list_plans_full,
            commands::get_plan_full,
            commands::save_clipboard_image,
            commands::save_clipboard_file,
            commands::materialize_dropped_files,
            clipboard_files::read_clipboard_files,
            clipboard_files::read_clipboard_text,
            commands::save_temp_file,
            commands::read_html_preview_file,
            commands::read_local_preview_file,
            commands::copy_image_to_clipboard,
            commands::check_for_update,
            commands::install_app_update,
            commands::get_update_status,
            commands::open_folder,
            commands::open_url,
            commands::test_notification,
            commands::notify_permission_state,
            commands::establish_notify_permission,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            commands::mux_commands::mux_get_state,
            commands::mux_commands::mux_create_workspace,
            commands::mux_commands::mux_set_active_workspace,
            commands::mux_commands::mux_close_workspace,
            commands::mux_commands::mux_rename_workspace,
            commands::mux_commands::mux_rename_team,
            commands::mux_commands::mux_close_team,
            commands::mux_commands::mux_add_team_member,
            commands::mux_commands::mux_move_tab,
            commands::mux_commands::mux_reorder_tab,
            commands::mux_commands::mux_reorder_child,
            commands::mux_commands::mux_reorder_workspace,
            commands::mux_commands::mux_create_tab,
            commands::mux_commands::mux_close_tab,
            commands::mux_commands::mux_set_active_tab,
            commands::mux_commands::mux_set_active_tab_and_workspace,
            commands::mux_commands::mux_rename_tab,
            commands::mux_commands::mux_close_tab,
            commands::mux_commands::mux_reopen_closed_tab,
            commands::mux_commands::mux_reopen_closed_group,
            commands::mux_commands::mux_restore_orphaned_tab,
            commands::mux_commands::mux_set_focused_tab,
            commands::mux_commands::mux_link_pty,
            commands::mux_commands::mux_rename_tab,
            commands::mux_commands::mux_update_tab_cwd,
            commands::mux_commands::mux_set_tab_agent_type,
            commands::mux_commands::mux_unlock_tab_agent_type,
            commands::mux_commands::mux_set_chat_view,
            commands::mux_commands::mux_set_permission_auto_confirm,
            commands::bridge_threads::bridge_thread_state,
            commands::bridge_threads::bridge_thread_open,
            commands::bridge_threads::bridge_thread_close,
            commands::mux_commands::detect_claude_session_id,
            commands::mux_commands::detect_tab_session_id,
            commands::mux_commands::mux_set_claude_session_id,
            commands::chat_start,
            commands::session_transcript,
            commands::chat_stop,
            commands::chat_backfill,
            commands::chat_send_text,
            commands::chat_send_status,
            commands::open_chat_backend_tab,
            commands::chatbackend_session_info,
            commands::chatbackend_switch_model,
            commands::transcribe_voice_attachment,
            commands::delayed_send_list,
            commands::delayed_send_create,
            commands::delayed_send_update,
            commands::delayed_send_delete,
            commands::delayed_send_send_now,
            commands::check_cli_installed,
            commands::refresh_provider_models,
            commands::install_provider_in_tab,
            commands::plugin_manager_snapshot,
            commands::plugin_manager_action,
            commands::plugin_glance,
            commands::plugin_bubble,
            commands::repo_watch_subscribe,
            commands::repo_watch_unsubscribe,
            commands::plugin_view_call,
            commands::plugin_tab_menu,
            commands::plugin_tab_menu_action,
            commands::mux_commands::build_agent_launch_command,
            commands::mux_commands::mux_spawn_tab,
            commands::mux_commands::mux_create_orchestration_pair,
            commands::mux_commands::mux_set_tab_role,
            commands::mux_commands::mux_clear_tab_role,
            commands::mux_commands::mux_make_orchestrator,
            commands::mux_commands::mux_make_planner,
            commands::mux_commands::mux_set_can_spawn,
            commands::mux_commands::mux_respond_prompt,
            commands::mux_commands::mux_batch,
            notification::notify,
            notification::push_channel_roster,
            notification::list_notifications,
            notification::mark_notification_read,
            notification::answer_notification,
            notification::clear_notifications,
            config::get_config,
            config::update_config,
            config::get_theme,
            config::list_themes,
            config::get_config_path,
            config::save_custom_theme,
            config::delete_custom_theme,
            config::get_themes_dir,
            commands::list_dir_completions,
            commands::check_provider_integration,
            commands::install_provider_integration,
            commands::uninstall_provider_integration,
            commands::restore_provider_integration,
            commands::check_provider_rule,
            commands::install_provider_rule,
            session::save_session,
            session::save_session_named,
            session::list_saved_sessions,
            session::load_session,
            session::load_scrollback_tail,
            commands::cron_list,
            commands::cron_get,
            commands::cron_create,
            commands::cron_update,
            commands::cron_delete,
            commands::cron_toggle,
            commands::cron_trigger,
            commands::cron_cancel,
            commands::cron_history,
            commands::cron_read_log,
            commands::services_list,
            commands::services_create,
            commands::services_get,
            commands::services_update,
            commands::services_delete,
            commands::services_regenerate_token,
            commands::activity_list,
            commands::services_smart_add_inspect,
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
            commands_mesh::mesh_set_enabled,
            commands_mesh::mesh_start_broadcast,
            commands_mesh::mesh_stop_broadcast,
            commands_mesh::mesh_scan_peers,
            commands_mesh::mesh_set_access,
            commands_mesh::mesh_peer_services_list,
            commands_mesh::mesh_inventory_list,
            commands_mesh::ordinary_tabs_list,
            commands_mesh::mesh_peer_tabs_list,
            commands_mesh::mesh_remote_tab_action,
            commands_mesh::mesh_peer_service_delete,
            commands_mesh::mesh_peer_cron_action,
            commands_mesh::mesh_peer_cron_update,
            commands_mesh::mesh_peer_cron_delete,
            commands_mesh::mesh_peer_cron_get,
            commands_mesh::mesh_peer_cron_history,
            keepawake::keepawake_start,
            keepawake::keepawake_stop,
            keepawake::keepawake_status,
            commands::usage_latest,
            commands::usage_history,
            commands::usage_daily_rollup,
            commands::usage_projection,
            commands::usage_refresh,
            crash::get_recovery_state,
        ])
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
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
            // One bundle of registry handles, shared by the startup loop and the hot-reload watcher (same register_plugin path, no drift).
            let cap_registries = plugin::js_runtime::CapabilityRegistries {
                secret: Some(std::sync::Arc::new(
                    crate::secrets::js_backend::AppSecretBackendRegistrar::new(secret_registry.clone())
                )),
                transcribe: transcribe_registry.clone(),
                glance: glance_registry.clone(),
                bubble: bubble_registry.clone(),
                view: view_registry.clone(),
                chatbackend: chatbackend_registry.clone(),
                chat_preprocessor: chat_preprocessor_registry.clone(),
                agent_command: agent_command_registry.clone(),
                webhook_receiver: webhook_registry.clone(),
                tab_menu: tab_menu_registry.clone(),
            };
            // VMs collected as they load, so the `ServiceContext` can be late-bound into each sandbox once it's built below.
            let mut plugin_vms: Vec<Arc<plugin::js_runtime::PluginVm>> = Vec::new();
            let mut plugin_manager = plugin::PluginManager::new();
            plugin_manager.set_registrar(agent_registry.clone());
            plugin_manager.set_event_sink(states.event_broadcaster.clone());

            // Bridge ContributionsChanged → frontend event + mem bridge rule refresh.
            {
                use tauri::Emitter;
                let mut sub = states.event_broadcaster.subscribe();
                let bridge_handle = app_handle.clone();
                let bridge_registry = agent_registry.clone();
                tauri::async_runtime::spawn(async move {
                    while let Ok(ev) = sub.recv().await {
                        if let crate::api::events::SystemEvent::ContributionsChanged { kind } =
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
                                crate::commands::mirror_skills_to_providers();
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

            // background refresh per provider off the async runtime (never gates startup; emits `models-updated`).
            {
                let providers = agent_registry.all_provider_arcs();
                for p in &providers {
                    agent::model_catalog::prime_from_cache(p.id());
                }
                let app_for_models = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    for provider in providers {
                        let id = provider.id().to_string();
                        if agent::model_catalog::refresh_models(provider.clone(), false)
                            .await
                            .is_some()
                        {
                            crate::api::events::emit_everywhere(
                                &app_for_models,
                                "models-updated",
                                serde_json::json!({ "provider": id }),
                            );
                        }
                        // Watched only after the first probe, because the path
                        // to watch is the one the resolver actually resolved.
                        agent::model_binary_watch::install(provider, app_for_models.clone());
                    }
                });
            }

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
            crate::service::plan::heartbeat::init_reflect_every(
                crate::config::load_config().orchestration.reflect.every_gates,
            );
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
            states.mesh.lock().build_hash = env!("BUILD_HASH").to_string();
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
                    crate::api::events::emit_everywhere(
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

            let svc_ctx = ServiceContext {
                mux: states.mux.clone(),
                pty: states.pty.clone(),
                output_buf: states.output_buffer.clone(),
                screen_state: states.screen_state.clone(),
                notif: states.notif.clone(),
                events: states.event_broadcaster.clone(),
                app_handle: app_handle.clone(),
                agent_tracker: states.agent_tracker.clone(),
                shell_caps: Arc::new(parking_lot::Mutex::new(Default::default())),
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
                mem_supervisor,
                activity: crate::mem::activity::ActivityRing::new(500),
                workers,
                plugin_activity: Arc::new(crate::plugin::activity::PluginActivityRegistry::new()),
                worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(
                    crate::config::memory_config().workers_per_provider,
                )),
                prompt_detector,
                pty_ownership: Default::default(),
                agent_run_pty_map: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
                remote_workers: Arc::new(remote_workers),
                bridge_outbound: bridge_outbound_tx,
                bridge_outbound_rx: Arc::new(Mutex::new(Some(bridge_outbound_rx))),
                bridge_supervisor: Arc::new(bridge::supervisor::BridgeSupervisor::new()),
                bridge_threads: Arc::new(codeterm_bridge::bridge::threads::ThreadService::new(
                    codeterm_bridge::bridge::threads::ThreadRegistry::load(
                        crate::service::bridge_threads::registry_path(),
                    ),
                )),
                restart_tracker: crate::service::agent_restart::new_restart_tracker(),
                provider_updates: Default::default(),
                chat_registry: crate::service::tab_chat::ChatRegistry::new(),
                delayed_send,
                secret_registry: secret_registry.clone(),
                secrets,
                transcribe_registry: transcribe_registry.clone(),
                glance_registry: glance_registry.clone(),
                bubble_registry: bubble_registry.clone(),
                repo_watch,
                view_registry: view_registry.clone(),
                chatbackend_registry: chatbackend_registry.clone(),
                chatbackend_reopen_guards: Arc::new(Default::default()),
                provider_registry,
                chat_preprocessor_registry: chat_preprocessor_registry.clone(),
                agent_command_registry: agent_command_registry.clone(),
                webhook_registry: webhook_registry.clone(),
                tab_menu_registry: tab_menu_registry.clone(),
                receiver_targets: Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(
                    crate::service::receiver_target::ReceiverTargetStore::default_path(),
                )),
                mux_emit_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                mux_emit_stats: Arc::new(crate::service::MuxEmitStats::default()),
                session_persist_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                services_store: Arc::new(services_store),
                tokens: Arc::new(crate::service::tokens::TokenStore::new()),
                pasteboard: crate::pasteboard::PasteboardState::new(),
                input_parser,
                usage_store,
                usage_activity: crate::usage::TabActivity::new(),
                user_last_activity,
                usage_poller: Arc::new(parking_lot::Mutex::new(None)),
                plan_watchers: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                loaded_plan_projects: Arc::new(parking_lot::Mutex::new(
                    std::collections::HashSet::new(),
                )),
                plan_startup_loaded: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                worker_identity_allocator: Arc::new(parking_lot::Mutex::new(
                    crate::service::WorkerPersonaReservations::default(),
                )),
                worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
                gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
            };
            let service_state: ServiceState = Arc::new(svc_ctx);
            service_state.plan_store.bind_task_key_port(Arc::new(
                crate::service::plan::task_key_port_impl::MuxTaskKeyPort::new(
                    service_state.mux.clone(),
                    service_state.app_handle.clone(),
                ),
            ));
            service_state
                .provider_updates
                .bind_execution_owner(&service_state);
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
                let _ = chatbackend_registry.start_discovery();
            }
            app.manage(service_state.clone());
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
            drop(startup_step);

            let startup_step = startup::StartupStep::logged("platform and server initialization");
            startup::setup_conpty_host(app);
            startup::install_bundled_cli(app);
            startup::start_servers(app, &states, &service_state);
            commands::ensure_integrations_for_all(&service_state.agent_registry, &states.config);
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
                        crate::api::events::emit_everywhere(
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" { return; }

                let app = window.app_handle();
                if let (Some(mux), Some(session)) = (
                    app.try_state::<MuxState>(),
                    app.try_state::<SessionState>(),
                ) {
                    let snapshot = mux.write().to_session_snapshot();
                    let saved = session.lock().save(&snapshot);
                    if let Err(e) = saved {
                        log::error!("Failed to save session: {}", e);
                    } else {
                        session::scrollback_sync::sync_scrollback_sidecars(&app);
                        log::info!("Session saved (close-to-tray)");
                    }
                }

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

            // Mark shutting down the moment exit is requested, so the liveness watchdog doesn't read the imminent API-down as a crash.
            if let tauri::RunEvent::ExitRequested { .. } = _event {
                crash::shutdown::mark_shutting_down();
            }

            // log doesn't survive into the next run. The updater's graceful_shutdown already does this on the in-place upgrade path; this catches every other shutdown route.
            if let tauri::RunEvent::Exit = _event {
                crash::exit::handle_run_exit(_app);
            }
        });
}
