use crate::crash;
use crate::startup;
use std::sync::Arc;
use tauri::Manager;

pub fn run(
    context: tauri::Context<codeterm_host_runtime::app_runtime::Runtime>,
    register_commands: fn(
        tauri::Builder<codeterm_host_runtime::app_runtime::Runtime>,
    ) -> tauri::Builder<codeterm_host_runtime::app_runtime::Runtime>,
) {
    codeterm_service_core::async_runtime::install(tauri::async_runtime::handle().inner().clone());
    // Never automatic: only an explicit `repair-user-path` argv reaches it.
    startup::run_user_path_repair_if_invoked();

    // Forward CLI-shaped argv to the bundled CLI before Tauri/logging init, otherwise GUI single-instance silently swallows the args (issue #51).
    startup::forward_to_cli_if_invoked();

    let path_owner =
        match codeterm_service_core::ResolvedInstancePaths::current().and_then(|paths| {
            codeterm_service_core::PathOwnership::try_acquire(
                paths,
                &domios_server::ownership_registry()?,
            )
        }) {
            Ok(owner) => owner,
            Err(error) => {
                eprintln!("Domios startup refused before initialization: {error}");
                return;
            }
        };

    // crash originates (tokio worker, Tauri main, FFI thread, signal handler).
    crash::install();

    startup::setup_logging();

    log::info!("CodeTerm starting...");
    let mut builder = codeterm_host_runtime::app_runtime::builder();
    if startup::desktop_lifecycle::single_instance_enabled(
        cfg!(debug_assertions),
        &context.config().identifier,
    ) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            startup::desktop_lifecycle::activate(app);
        }));
    }

    let builder = builder
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

    let builder = builder.plugin(tauri_plugin_deep_link::init());
    register_commands(builder)
        .setup(move |app| {
            #[cfg(debug_assertions)]
            tauri::async_runtime::spawn(codeterm_service_core::startup::runtime_diagnostics::watch("main"));
            app.manage(path_owner);
            app.manage(codeterm_service_core::acquire_instance_lifetime()?);
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

            let app_handle = app.handle().clone();
            codeterm_service_core::reap_orphaned_webview2();
            let running = domios_server::compose(
                |_| Arc::new(crate::host_ports::TauriUiEvents(app_handle.clone())),
                Arc::new(crate::host_ports::TauriShell(app_handle.clone())),
                app.config().build.dev_url.clone(),
                app.path().resource_dir().ok(),
            )?;
            let states = &running.states;
            app.manage(states.storage.clone());
            app.manage(states.config.clone());
            app.manage(states.pty.clone());
            app.manage(states.mux.clone());
            app.manage(states.notif.clone());
            app.manage(states.session.clone());
            app.manage(states.output_buffer.clone());
            app.manage(states.screen_state.clone());
            app.manage(states.agent_tracker.clone());
            app.manage(states.report_store.clone());
            app.manage(states.event_broadcaster.clone());
            app.manage(states.update.clone());
            app.manage(states.mesh.clone());
            app.manage(states.keepawake.clone());
            app.manage(states.resize_owner.clone());
            app.manage(states.rest_api.clone());
            app.manage(running.service.clone());
            app.manage(running.service.ext::<codeterm_service_core::CronState>().clone());
            app.manage(running.service.ext::<codeterm_service_core::CronWake>().clone());
            if let Some(bundles) = &running.frontend_bundles {
                app.manage(bundles.clone());
            }
            assert!(running.service.register_host_extension(Arc::new(
                codeterm_commands::application_host::ApplicationHostState(Arc::new(
                    codeterm_commands::application_host::DesktopApplicationHost {
                        app: app_handle.clone(), update_state: states.update.clone(),
                    }
                ))
            )).is_ok());
            app.manage(startup::desktop_lifecycle::DesktopLifecycle::default());
            startup::frontend_shell_initialize(&app_handle)?;
            startup::desktop_lifecycle::show_or_create(&app_handle)?;
            crate::deep_links::register_deep_links(&app_handle, running.service.clone())?;
            startup::frontend_shell_start(app_handle.clone(), states.rest_api.clone());
            startup::build_system_tray(app)?;
            #[cfg(target_os = "macos")]
            crash::macos_quit::install_quit_hook();
            #[cfg(unix)]
            {
                let signal_app = app_handle.clone();
                crash::shutdown::install_unix_signal_handlers(move || {
                    crash::exit::graceful_exit_cleanup(&signal_app);
                });
            }
            Ok(())
        })
        .on_window_event(startup::handle_window_event)
        .build(startup::frontend_shell_application_context(context))
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                startup::desktop_lifecycle::activate(app);
            }
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &event
            {
                if label == "main" {
                    startup::desktop_lifecycle::destroyed(app);
                }
            }
            if let tauri::RunEvent::ExitRequested { code, api, .. } = &event {
                if startup::desktop_lifecycle::retain_daemon(app, *code) {
                    api.prevent_exit();
                    log::debug!("[desktop] prevented implicit last-window daemon exit");
                } else {
                    log::info!("[shutdown] explicit RunEvent::ExitRequested received");
                    crash::shutdown::mark_shutting_down();
                }
            }

            // log doesn't survive into the next run. The updater's graceful_shutdown already does this on the in-place upgrade path; this catches every other shutdown route.
            if let tauri::RunEvent::Exit = event {
                crash::exit::handle_run_exit(app);
            }
        });
    log::info!("[shutdown] Tauri event loop returned");
}
