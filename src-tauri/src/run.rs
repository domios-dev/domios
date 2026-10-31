mod development_context;
mod quit_plan;

use crate::crash;
use crate::startup;
use codeterm_types::server_bootstrap::{InstanceSelector, LaunchOptions, ServerComponentManifest};
use quit_plan::{ExitGate, QuitFeedback, QuitOrigin, QuitResolution, QuitStart};
use std::{io, io::Read, path::PathBuf};
use tauri::Manager;

pub fn run(
    mut context: tauri::Context<codeterm_client_shell::app_runtime::Runtime>,
    register_commands: fn(
        tauri::Builder<codeterm_client_shell::app_runtime::Runtime>,
    ) -> tauri::Builder<codeterm_client_shell::app_runtime::Runtime>,
) {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let startup_options = match crate::preflight_client_arguments(&arguments) {
        Ok(crate::ClientArgumentPreflight::ExistingDispatch) => {
            startup::run_user_path_repair_if_invoked();
            startup::forward_to_cli_if_invoked();
            crate::ClientStartupOptions::default()
        }
        Ok(crate::ClientArgumentPreflight::Native(options)) => options,
        Err(error) => {
            eprintln!("Domios client arguments refused: {error:?}");
            return;
        }
    };
    let selector = match client_instance_selector() {
        Ok(selector) => selector,
        Err(error) => {
            eprintln!("Domios client selector refused: {error}");
            return;
        }
    };
    if let Err(error) = development_context::configure(
        context.config_mut(),
        cfg!(debug_assertions),
        &selector,
        |key| std::env::var(key).ok(),
    ) {
        eprintln!("Domios development configuration refused: {error}");
        return;
    }
    let relaunch_env = codeterm_paths::relaunch_env::ApplicationRelaunchEnv::capture();
    install_client_selector(&selector);
    let mut builder = codeterm_client_shell::app_runtime::builder().manage(relaunch_env);
    if startup::desktop_lifecycle::single_instance_enabled(
        cfg!(debug_assertions),
        &context.config().identifier,
    ) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            startup::desktop_lifecycle::activate(app);
        }));
    }

    let builder = builder
        .register_uri_scheme_protocol(startup::frontend_shell::initial_document::SCHEME, |_, _| {
            startup::frontend_shell::initial_document::protocol_response()
        })
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
            let app_handle = app.handle().clone();
            crate::install_client_startup_options(&app_handle, startup_options)?;
            startup::frontend_shell_initialize(&app_handle)?;
            app.manage(startup::desktop_lifecycle::DesktopLifecycle::default());
            startup::starting_window::show(&app_handle)?;
            crate::host_ports::update_pair::install(&app_handle);
            startup::build_system_tray(app)?;
            #[cfg(target_os = "macos")]
            {
                let _ = MAC_CLIENT_APP.set(app_handle.clone());
                codeterm_crash::crash::macos_quit::install_quit_hook_with(mac_client_quit);
            }
            #[cfg(unix)]
            install_client_signal_handlers(app_handle.clone());
            let instance = selector.clone();
            tauri::async_runtime::spawn(async move {
                let result = initialize_client_installation(app_handle.clone(), instance).await;
                match result {
                    Ok(attached) => {
                        log::debug!(
                            "[client] server attachment ready; epoch={} native={:?}",
                            attached.ready.epoch,
                            attached.native
                        );
                        #[cfg(debug_assertions)]
                        let readiness = matches!(
                            attached.native,
                            crate::host_ports::native_client::NativeClientStatus::Attached
                        )
                        .then(|| {
                            serde_json::json!({
                                "protocol": attached.ready.protocol,
                                "client_pid": std::process::id(),
                                "server_pid": attached.ready.process.pid,
                                "epoch": attached.ready.epoch,
                                "api_address": attached.ready.api_address.to_string(),
                            })
                        });
                        let handle = app_handle.clone();
                        if let Err(error) = app_handle.run_on_main_thread(move || {
                            if let Err(error) = start_attached_frontend(&handle) {
                                crate::host_ports::native_client::startup_refused(
                                    crate::host_ports::native_client::ClientStartupStage::Frontend,
                                );
                                log::error!("[client] frontend initialization refused: {error}");
                                if let Some(window) = handle.get_webview_window("starting") {
                                    startup::frontend_shell::show_start_failure(
                                        &window,
                                        &error.to_string(),
                                    );
                                }
                                return;
                            }
                            #[cfg(debug_assertions)]
                            if let Some(readiness) = readiness {
                                eprintln!("DOMIOS_DEV_ATTACHMENT {readiness}");
                            }
                        }) {
                            crate::host_ports::native_client::startup_refused(
                                crate::host_ports::native_client::ClientStartupStage::Frontend,
                            );
                            log::error!("[client] frontend scheduling refused: {error}");
                            if let Some(window) = app_handle.get_webview_window("starting") {
                                startup::frontend_shell::show_start_failure(
                                    &window,
                                    &error.to_string(),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        crate::host_ports::native_client::startup_refused(
                            crate::host_ports::native_client::ClientStartupStage::Startup,
                        );
                        log::error!("[client] server attachment refused: {error}");
                        let handle = app_handle.clone();
                        if let Err(error) = app_handle.run_on_main_thread(move || {
                            if let Some(window) = handle
                                .get_webview_window("starting")
                                .or_else(|| handle.get_webview_window("main"))
                            {
                                startup::frontend_shell::show_start_failure(
                                    &window,
                                    error.message(),
                                );
                            }
                        }) {
                            log::error!(
                                "[client] unavailable frontend scheduling refused: {error}"
                            );
                        }
                    }
                }
            });
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
                if client_exit_confirmed() || code.is_some_and(|code| code != 0) {
                    return;
                }
                api.prevent_exit();
                if code.is_none()
                    && !crash::shutdown::is_shutting_down()
                    && startup::desktop_lifecycle::retain_daemon(app, *code)
                {
                    log::debug!("[desktop] retained client after implicit last-window exit");
                } else {
                    request_client_quit(app);
                }
            }
        });
    log::info!("[shutdown] Tauri event loop returned");
}

fn client_exit_confirmed() -> bool {
    crash::exit::client_exit_status()
        .is_some_and(|status| status.phase == crash::exit::ClientExitPhase::Confirmed)
}

fn request_client_quit(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let _ = request_client_quit_exact(app, None);
}

pub(crate) fn request_client_quit_exact(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    expected: Option<codeterm_types::app_quit::QuitSnapshot>,
) -> Result<(), codeterm_types::app_quit::QuitRefusal> {
    use crate::host_ports::{
        native_client::{ExactServerShutdownResult, NativeHostClientSlot},
        ServerApiAttachmentSlot,
    };
    use codeterm_types::app_quit::QuitRefusal;
    let origin = if expected.is_some() {
        QuitOrigin::Remote
    } else {
        QuitOrigin::Local
    };
    let native = app
        .try_state::<NativeHostClientSlot>()
        .map(|slot| slot.inner().clone());
    let api = app
        .try_state::<ServerApiAttachmentSlot>()
        .map(|slot| slot.inner().clone());
    let attached = native.is_some() && api.as_ref().is_some_and(|api| api.current().is_some());
    let gate = match crash::exit::client_exit_status().map(|status| status.phase) {
        None => ExitGate::Idle,
        Some(crash::exit::ClientExitPhase::Requested) => ExitGate::Requested,
        Some(crash::exit::ClientExitPhase::Uncertain) => ExitGate::Uncertain,
        Some(crash::exit::ClientExitPhase::Confirmed) => ExitGate::Confirmed,
    };
    match quit_plan::plan_quit_start(origin, gate, attached) {
        QuitStart::RequestServerShutdown => {}
        QuitStart::CloseClientOnly => {
            close_client_only(app);
            return Ok(());
        }
        QuitStart::Busy(feedback) => {
            show_quit_feedback(app, feedback);
            return Err(QuitRefusal::Busy);
        }
        QuitStart::Unavailable => {
            log::debug!("[client] quit refused before dispatch: server attachment unavailable");
            show_quit_feedback(app, QuitFeedback::Refused);
            return Err(QuitRefusal::Unavailable);
        }
    }
    let (Some(native), Some(api)) = (native, api) else {
        return Err(QuitRefusal::Unavailable);
    };
    let Ok(ticket) = crash::exit::begin_client_exit(crash::exit::ClientExitReason::ExplicitQuit)
    else {
        show_quit_feedback(app, QuitFeedback::Waiting);
        return Err(QuitRefusal::Busy);
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = native
            .request_exact_server_shutdown(&api, expected.as_ref())
            .await;
        match &result {
            ExactServerShutdownResult::Confirmed {
                server,
                client_generation,
            } => {
                log::debug!("[client] exact server exit observed; epoch={} client_generation={client_generation}", server.epoch);
            }
            ExactServerShutdownResult::RefusedBeforeDispatch(reason) => {
                crash::exit::refuse_client_exit_before_dispatch(ticket);
                log::debug!("[client] quit refused before dispatch: {reason:?}");
            }
            ExactServerShutdownResult::UncertainAfterDispatch(reason) => {
                crash::exit::mark_client_exit_uncertain(ticket);
                log::debug!("[client] exact server exit remains uncertain: {reason:?}");
            }
        }
        quit_plan::finish_quit(
            origin,
            &result,
            || crash::exit::confirm_client_exit(ticket),
            |resolution| match resolution {
                QuitResolution::ExitClient => app.exit(0),
                QuitResolution::CloseClientOnly => close_client_only(&app),
                QuitResolution::Feedback(feedback) => show_quit_feedback(&app, feedback),
            },
        );
    });
    Ok(())
}

fn close_client_only(app: &codeterm_client_shell::app_runtime::AppHandle) {
    if crash::exit::confirm_client_only_exit() {
        crate::host_ports::native_client::record_client_only_quit();
        app.exit(0);
    }
}

#[derive(Default)]
struct QuitFeedbackState {
    generation: u64,
    current: Option<(u64, QuitFeedback)>,
}

static LAST_QUIT_FEEDBACK: std::sync::Mutex<QuitFeedbackState> =
    std::sync::Mutex::new(QuitFeedbackState {
        generation: 0,
        current: None,
    });

fn new_quit_feedback(state: &mut QuitFeedbackState, next: QuitFeedback) -> Option<u64> {
    if state.current.is_some_and(|(_, feedback)| feedback == next) {
        return None;
    }
    state.generation = state.generation.checked_add(1)?;
    state.current = Some((state.generation, next));
    Some(state.generation)
}

fn finish_quit_feedback(state: &mut QuitFeedbackState, generation: u64) {
    if state
        .current
        .is_some_and(|(current, _)| current == generation)
    {
        state.current = None;
    }
}

fn complete_quit_dialog(generation: u64) {
    let mut state = LAST_QUIT_FEEDBACK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    finish_quit_feedback(&mut state, generation);
}

fn show_quit_feedback(app: &codeterm_client_shell::app_runtime::AppHandle, feedback: QuitFeedback) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let generation = {
        let mut previous = LAST_QUIT_FEEDBACK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        new_quit_feedback(&mut previous, feedback)
    };
    let Some(generation) = generation else {
        return;
    };
    let message = match feedback {
        QuitFeedback::Waiting => "Domios is waiting for the current exit operation. The app remains open until completion is confirmed.",
        QuitFeedback::Refused => "This quit request was refused before being sent to the server. The app remains open.",
        QuitFeedback::Uncertain => "Domios could not confirm that the server finished saving and shutting down. The app remains open. Do not repeat shutdown while its outcome is uncertain.",
        QuitFeedback::OfferCloseAfterRefusal => "Domios could not send the shutdown request, so the server was not stopped. You can close this window only and leave the server as it is.",
        QuitFeedback::OfferCloseAfterUncertainty => "Domios could not confirm that the server finished saving and shutting down. You can close this window only; the shutdown request will not be sent again.",
    };
    let offers_close = matches!(
        feedback,
        QuitFeedback::OfferCloseAfterRefusal | QuitFeedback::OfferCloseAfterUncertainty
    );
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let dialog = handle
            .dialog()
            .message(message)
            .title("Domios shutdown")
            .kind(MessageDialogKind::Warning);
        let dialog = if offers_close {
            dialog.buttons(MessageDialogButtons::OkCancelCustom(
                "Close window only".into(),
                "Keep open".into(),
            ))
        } else {
            dialog
        };
        let app = handle.clone();
        dialog.show(move |close| {
            complete_quit_dialog(generation);
            if offers_close && close {
                close_client_only(&app);
            }
        });
    }) {
        complete_quit_dialog(generation);
        log::error!("[client] shutdown feedback could not be shown: {error}");
    }
}

#[cfg(target_os = "macos")]
static MAC_CLIENT_APP: std::sync::OnceLock<codeterm_client_shell::app_runtime::AppHandle> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
fn mac_client_quit() -> codeterm_crash::crash::macos_quit::QuitDecision {
    use codeterm_crash::crash::macos_quit::QuitDecision;
    if client_exit_confirmed() {
        QuitDecision::Proceed
    } else {
        if let Some(app) = MAC_CLIENT_APP.get() {
            request_client_quit(app);
        }
        QuitDecision::Cancel
    }
}

#[cfg(unix)]
fn install_client_signal_handlers(app: codeterm_client_shell::app_runtime::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let result = async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            tokio::select! {
                result = tokio::signal::ctrl_c() => result,
                received = term.recv() => received.ok_or_else(|| io::Error::other("Client signal channel closed")),
            }
        }.await;
        if let Err(error) = result {
            log::error!("[client] signal observation failed: {error}");
            return;
        }
        let Ok(ticket) = crash::exit::begin_client_exit(crash::exit::ClientExitReason::UnixSignal)
        else {
            return;
        };
        if crash::exit::confirm_client_exit(ticket) {
            app.exit(0);
        }
    });
}

fn start_attached_frontend(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    crate::deep_links::register_deep_links(app)?;
    startup::desktop_lifecycle::show_or_create(app)?;
    startup::frontend_shell_start(app.clone());
    Ok(())
}

pub(crate) async fn initialize_client_installation(
    app: codeterm_client_shell::app_runtime::AppHandle,
    instance: InstanceSelector,
) -> Result<
    crate::host_ports::native_client::NativeClientInitialization,
    crate::host_ports::native_client::ClientStartupFailure,
> {
    let root = client_resource_root(&app).map_err(|error| {
        crate::host_ports::native_client::startup_io_error(
            crate::host_ports::native_client::ClientStartupStage::ResourceRoot,
            &error,
        );
        format!("Installed client resource directory unavailable: {error}")
    })?;
    let inputs = tokio::task::spawn_blocking(move || {
        crate::host_ports::native_client::startup_started(
            crate::host_ports::native_client::ClientStartupStage::Inventory,
        );
        load_client_components(root, instance)
    })
    .await
    .map_err(|_| "Installed client resource verification was interrupted".to_string())?
    .map_err(|error| format!("Installed client resources refused: {error}"))?;
    let (manifest, resources, options) = inputs;
    crate::host_ports::native_client::initialize_client(app, manifest, resources, options).await
}

pub(crate) async fn retry_client_installation(
    app: codeterm_client_shell::app_runtime::AppHandle,
) -> Result<
    crate::host_ports::native_client::NativeClientInitialization,
    crate::host_ports::native_client::ClientStartupFailure,
> {
    let instance = client_instance_selector().map_err(|error| error.to_string())?;
    initialize_client_installation(app, instance).await
}

fn client_resource_root(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(directory) = std::env::var_os("DOMIOS_UPDATE_ROLLBACK") {
        return Ok(std::path::PathBuf::from(directory).join("rollback/resources"));
    }
    app.path().resource_dir().map_err(io::Error::other)
}

fn load_client_components(
    resources: PathBuf,
    instance: InstanceSelector,
) -> io::Result<(ServerComponentManifest, PathBuf, LaunchOptions)> {
    let resources = resources.canonicalize()?;
    let manifest_file = resources.join("server-component.json");
    if !manifest_file.symlink_metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Installed component inventory must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(manifest_file)?
        .take(8193)
        .read_to_end(&mut bytes)?;
    let manifest = parse_client_components(&bytes)?;
    let options = LaunchOptions {
        instance,
        resources: resources.clone(),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    };
    options.validate(cfg!(debug_assertions))?;
    Ok((manifest, resources, options))
}

fn parse_client_components(bytes: &[u8]) -> io::Result<ServerComponentManifest> {
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Component inventory exceeds bound",
        ));
    }
    serde_json::from_slice(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid installed component inventory",
        )
    })
}

fn client_instance_selector() -> io::Result<InstanceSelector> {
    let instance = std::env::var_os("CODETERM_DEV_INSTANCE");
    let port = std::env::var_os("CODETERM_DEV_API_PORT");
    select_client_instance(cfg!(debug_assertions), instance.as_deref(), port.as_deref())
}

fn select_client_instance(
    development: bool,
    instance: Option<&std::ffi::OsStr>,
    port: Option<&std::ffi::OsStr>,
) -> io::Result<InstanceSelector> {
    if !development {
        return Ok(InstanceSelector::Production);
    }
    let instance = match instance {
        None => None,
        Some(value) => match value.to_str() {
            Some("") => None,
            Some(value) => Some(value.to_owned()),
            None => return Err(io::Error::other("Invalid explicit development instance")),
        },
    };
    let port = port
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            io::Error::other("Development client requires explicit CODETERM_DEV_API_PORT")
        })?
        .parse::<u16>()
        .map_err(|_| io::Error::other("Invalid explicit development port"))?;
    let instance = InstanceSelector::Development { instance, port };
    LaunchOptions {
        instance: instance.clone(),
        resources: PathBuf::new(),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    }
    .validate(true)?;
    Ok(instance)
}

fn install_client_selector(selector: &InstanceSelector) {
    for key in [
        "CODETERM_API_URL",
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_SOCKET_PATH",
        "CODETERM_INSTANCE",
        "CODETERM_DATA_DIR",
        "CODETERM_DEV",
        "CODETERM_DEV_INSTANCE",
        "CODETERM_DEV_API_PORT",
    ] {
        std::env::remove_var(key);
    }
    if let InstanceSelector::Development {
        instance: Some(instance),
        ..
    } = selector
    {
        std::env::set_var("CODETERM_INSTANCE", instance);
    }
}

#[cfg(test)]
mod process_split_contract_client_composition {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn quit_feedback_only_repeats_for_a_new_state_transition() {
        let mut previous = QuitFeedbackState::default();
        for state in [
            QuitFeedback::Refused,
            QuitFeedback::Waiting,
            QuitFeedback::Uncertain,
        ] {
            assert!(new_quit_feedback(&mut previous, state).is_some());
            assert!(new_quit_feedback(&mut previous, state).is_none());
        }
        assert!(new_quit_feedback(&mut previous, QuitFeedback::Refused).is_some());
    }

    #[test]
    fn distinct_quit_dialog_attempts_repeat_feedback_without_stale_completion() {
        let mut state = QuitFeedbackState::default();
        let first = new_quit_feedback(&mut state, QuitFeedback::Refused).unwrap();
        assert!(new_quit_feedback(&mut state, QuitFeedback::Refused).is_none());
        finish_quit_feedback(&mut state, first);
        let second = new_quit_feedback(&mut state, QuitFeedback::Refused).unwrap();
        assert_ne!(first, second);
        finish_quit_feedback(&mut state, first);
        assert!(new_quit_feedback(&mut state, QuitFeedback::Refused).is_none());
        finish_quit_feedback(&mut state, second);
        assert!(new_quit_feedback(&mut state, QuitFeedback::Refused).is_some());
    }

    fn manifest() -> serde_json::Value {
        serde_json::json!({
            "schema": 2, "server_build": "independent-server-build",
            "server_executable": "domios-server/domios-server",
            "server_sha256": "a".repeat(64), "client_identity": {"kind":"sha256", "sha256":"b".repeat(64)},
            "api_version": 3, "bootstrap_protocol": codeterm_types::server_bootstrap::BOOTSTRAP_PROTOCOL,
        })
    }

    #[test]
    fn strict_bounded_inventory_retains_independent_server_build() {
        let value = manifest();
        let bytes = serde_json::to_vec(&value).unwrap();
        let parsed = parse_client_components(&bytes).unwrap();
        assert_eq!(parsed.server_build, value["server_build"].as_str().unwrap());
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        assert!(parse_client_components(&[]).is_err());
        assert!(parse_client_components(&vec![b' '; 8193]).is_err());
        let mut unknown = value.clone();
        unknown["fallback"] = true.into();
        assert!(parse_client_components(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("client_identity");
        assert!(parse_client_components(&serde_json::to_vec(&missing).unwrap()).is_err());
    }

    #[test]
    fn dev_diagnostics_publication_uses_captured_selector_not_scrubbed_environment() {
        let arguments = [std::ffi::OsString::from(
            "--local-webview-diagnostics-port=9223",
        )];
        assert!(matches!(
            crate::preflight_client_arguments(&arguments).unwrap(),
            crate::ClientArgumentPreflight::Native(_)
        ));
        let selector = select_client_instance(true, None, Some(OsStr::new("7686"))).unwrap();
        assert!(crate::host_ports::webview_diagnostics::dev_locator_enabled(
            true, &selector
        ));
        assert!(!crate::host_ports::webview_diagnostics::dev_locator_enabled(false, &selector));
        assert!(
            !crate::host_ports::webview_diagnostics::dev_locator_enabled(
                true,
                &InstanceSelector::Production
            )
        );
    }

    #[test]
    fn production_selection_ignores_inherited_launch_selectors() {
        assert!(matches!(
            select_client_instance(false, Some(OsStr::new("foreign")), Some(OsStr::new("7686")))
                .unwrap(),
            InstanceSelector::Production
        ));
    }

    #[test]
    fn development_requires_explicit_valid_nonproduction_port_and_name() {
        assert!(select_client_instance(true, None, None).is_err());
        for port in ["0", "7685", "65536", "invalid"] {
            assert!(select_client_instance(true, None, Some(OsStr::new(port))).is_err());
        }
        assert!(select_client_instance(
            true,
            Some(OsStr::new("../foreign")),
            Some(OsStr::new("7880"))
        )
        .is_err());
        assert!(matches!(
            select_client_instance(true, Some(OsStr::new("")), Some(OsStr::new("7686"))).unwrap(),
            InstanceSelector::Development {
                instance: None,
                port: 7686
            }
        ));
        assert!(matches!(
            select_client_instance(true, Some(OsStr::new("split-oracle-client")), Some(OsStr::new("7880"))).unwrap(),
            InstanceSelector::Development { instance: Some(name), port: 7880 } if name == "split-oracle-client"
        ));
    }
}
