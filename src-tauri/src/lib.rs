mod command_registration;
mod host_ports;
mod run;
mod startup;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    if let Some(code) = codeterm_update::debian::handoff::entry() {
        std::process::exit(code);
    }
    #[cfg(target_os = "linux")]
    let _rollback_lease = std::env::var_os("DOMIOS_UPDATE_ROLLBACK").map(|directory| {
        codeterm_update::debian::handoff::rollback_leases(std::path::Path::new(&directory))
            .unwrap_or_else(|_| std::process::exit(1))
    });
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_build_info::install_version(env!("CARGO_PKG_VERSION"));
    run::run(app_context(), command_registration::register_commands);
}

// macOS embeds Info.plist under one fixed symbol per expansion, so the crate expands it once.
pub(crate) fn app_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

mod crash;
mod os_notify;

mod deep_links;
#[cfg(windows)]
mod native_resize;
mod webview_watchdog;

const LOCAL_WEBVIEW_DIAGNOSTICS_ARGUMENT: &str = "--local-webview-diagnostics-port";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientArgumentPreflight {
    ExistingDispatch,
    Native(ClientStartupOptions),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LocalWebviewDiagnosticsMode {
    #[default]
    Disabled,
    RequestedPort(u16),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClientStartupOptions {
    pub(crate) local_webview_diagnostics: LocalWebviewDiagnosticsMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientArgumentError {
    DiagnosticsMustBeStandalone,
    InvalidDiagnosticsSyntax,
    InvalidDiagnosticsPort,
}

pub(crate) fn preflight_client_arguments(
    arguments: &[std::ffi::OsString],
) -> Result<ClientArgumentPreflight, ClientArgumentError> {
    let option_prefix = format!("{LOCAL_WEBVIEW_DIAGNOSTICS_ARGUMENT}=");
    let has_diagnostics_argument = arguments.iter().any(|argument| {
        argument.to_str().is_some_and(|argument| {
            argument == LOCAL_WEBVIEW_DIAGNOSTICS_ARGUMENT || argument.starts_with(&option_prefix)
        })
    });

    if !has_diagnostics_argument {
        return Ok(ClientArgumentPreflight::ExistingDispatch);
    }
    if arguments.len() != 1 {
        return Err(ClientArgumentError::DiagnosticsMustBeStandalone);
    }

    let argument = arguments[0]
        .to_str()
        .ok_or(ClientArgumentError::InvalidDiagnosticsSyntax)?;
    let port = argument
        .strip_prefix(&option_prefix)
        .ok_or(ClientArgumentError::InvalidDiagnosticsSyntax)?
        .parse::<u16>()
        .map_err(|_| ClientArgumentError::InvalidDiagnosticsPort)?;
    if port < 1024 {
        return Err(ClientArgumentError::InvalidDiagnosticsPort);
    }

    Ok(ClientArgumentPreflight::Native(ClientStartupOptions {
        local_webview_diagnostics: LocalWebviewDiagnosticsMode::RequestedPort(port),
    }))
}

pub(crate) fn install_client_startup_options(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    options: ClientStartupOptions,
) -> Result<(), &'static str> {
    use tauri::Manager as _;

    app.manage(options)
        .then_some(())
        .ok_or("Client startup options were already installed")
}

#[cfg(test)]
mod process_split_contract_client_arguments {
    use super::*;

    #[test]
    fn diagnostics_rejects_mixed_cli_arguments_before_forwarding() {
        let arguments = [
            std::ffi::OsString::from("workspace"),
            std::ffi::OsString::from("--local-webview-diagnostics-port=9222"),
        ];

        assert_eq!(
            preflight_client_arguments(&arguments),
            Err(ClientArgumentError::DiagnosticsMustBeStandalone)
        );
    }

    #[test]
    fn absent_diagnostics_preserves_the_existing_cli_dispatch_path() {
        let arguments = [
            std::ffi::OsString::from("workspace"),
            std::ffi::OsString::from("--help"),
        ];

        assert_eq!(
            preflight_client_arguments(&arguments),
            Ok(ClientArgumentPreflight::ExistingDispatch)
        );
    }

    #[test]
    fn diagnostics_accepts_only_the_standalone_port_option() {
        let arguments = [std::ffi::OsString::from(
            "--local-webview-diagnostics-port=9222",
        )];

        assert_eq!(
            preflight_client_arguments(&arguments),
            Ok(ClientArgumentPreflight::Native(ClientStartupOptions {
                local_webview_diagnostics: LocalWebviewDiagnosticsMode::RequestedPort(9222),
            }))
        );
    }
}
