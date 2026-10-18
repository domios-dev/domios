use codeterm_service_core::service::account::deep_links;

pub fn register_deep_links(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
    service: codeterm_service_core::service::ServiceState,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_deep_link::DeepLinkExt;
    if !production_instance(
        cfg!(debug_assertions),
        std::env::var("CODETERM_DEV").ok().as_deref(),
        std::env::var("CODETERM_INSTANCE").ok().as_deref(),
        std::env::var("CODETERM_API_PORT").ok().as_deref(),
        &app.config().identifier,
    ) {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let registered = app.deep_link().is_registered("domios")?;
    #[cfg(target_os = "linux")]
    let registered = linux_registration_matches(app)?;
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if registration_needs_repair(true, registered) {
        app.deep_link().register("domios")?;
    }
    #[cfg(target_os = "macos")]
    macos::repair_registration()?;
    let callback_app = app.clone();
    let dispatch = move |urls: Vec<url::Url>| {
        let app = callback_app.clone();
        let service = service.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            for url in urls {
                match deep_links::deep_link_action(&url) {
                    Some(deep_links::DeepLinkAction::BillingReturn { status }) => {
                        log::debug!(
                            "[billing] return received, status flag present: {}",
                            status.is_some()
                        );
                        let _ = service
                            .refresh_billing(codeterm_billing::RefreshEvent::CheckoutReturned)
                            .await;
                        crate::startup::desktop_lifecycle::activate(&app);
                    }
                    Some(deep_links::DeepLinkAction::AccountCallback(callback)) => {
                        if let Ok(account) = service.account().await {
                            let _ = account.complete(callback).await;
                        }
                    }
                    None => {}
                }
            }
        });
    };
    if let Some(urls) = app.deep_link().get_current()? {
        dispatch(urls);
    }
    app.deep_link()
        .on_open_url(move |event| dispatch(event.urls()));
    Ok(())
}

fn production_instance(
    debug: bool,
    dev: Option<&str>,
    instance: Option<&str>,
    port: Option<&str>,
    identifier: &str,
) -> bool {
    !debug
        && dev != Some("1")
        && instance.is_none_or(|value| value.trim().is_empty())
        && port.is_none_or(|port| port == "7685")
        && identifier == "com.codeterm.app"
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn registration_needs_repair(production: bool, matches_executable: bool) -> bool {
    production && !matches_executable
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_repair_requires_production_and_a_mismatched_handler() {
        assert!(registration_needs_repair(true, false));
        assert!(!registration_needs_repair(true, true));
        assert!(!registration_needs_repair(false, false));
        assert!(!registration_needs_repair(false, true));
    }

    #[test]
    fn only_default_production_instance_can_register_or_handle_production_links() {
        assert!(production_instance(
            false,
            None,
            None,
            None,
            "com.codeterm.app"
        ));
        for (debug, dev, instance, port, identifier) in [
            (true, None, None, None, "com.codeterm.app"),
            (false, Some("1"), None, None, "com.codeterm.app"),
            (false, None, Some("alternate"), None, "com.codeterm.app"),
            (false, None, None, Some("7686"), "com.codeterm.app"),
            (false, None, None, None, "com.codeterm.dev"),
        ] {
            assert!(!production_instance(debug, dev, instance, port, identifier));
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_registration_matches(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
) -> Result<bool, Box<dyn std::error::Error>> {
    use tauri::Manager;
    let output = std::process::Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/domios"])
        .output()?;
    let name = String::from_utf8_lossy(&output.stdout);
    let name = name.trim();
    if !output.status.success() || name.is_empty() || name.contains(['/', '\\']) {
        return Ok(false);
    }
    let mut roots = vec![app.path().data_dir()?];
    roots.extend(std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ));
    let desktop = roots
        .into_iter()
        .find_map(|root| std::fs::read_to_string(root.join("applications").join(name)).ok());
    let Some(desktop) = desktop else {
        return Ok(false);
    };
    let Some(command) = desktop.lines().find_map(|line| line.strip_prefix("Exec=")) else {
        return Ok(false);
    };
    let args = shell_words::split(command)?;
    let Some(command) = args.first() else {
        return Ok(false);
    };
    let bin = tauri::utils::platform::current_exe()?;
    let expected =
        std::path::PathBuf::from(app.env().appimage.unwrap_or_else(|| bin.into_os_string()));
    let executable = std::path::PathBuf::from(command);
    let executable = if executable.is_absolute() {
        Some(executable)
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|root| root.join(command))
                .find(|candidate| candidate.is_file())
        })
    };
    Ok(executable.and_then(|path| path.canonicalize().ok()) == Some(expected.canonicalize()?))
}
