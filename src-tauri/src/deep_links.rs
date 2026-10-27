use crate::host_ports::ServerApiAttachmentSlot;
use tauri::Manager;

pub fn register_deep_links(
    app: &codeterm_client_shell::app_runtime::AppHandle,
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
        tauri::async_runtime::spawn(async move {
            for url in urls {
                if is_focus_action(&url) {
                    crate::startup::desktop_lifecycle::activate(&app);
                    continue;
                }
                let attachment = app
                    .try_state::<ServerApiAttachmentSlot>()
                    .and_then(|slot| slot.current());
                if let (Some(attachment), Some(status)) =
                    (attachment.as_ref(), billing_return_status(&url))
                {
                    log::debug!(
                        "[billing] return received, status flag present: {}",
                        status.is_some()
                    );
                    let _ = attachment.refresh_plan().await;
                    crate::startup::desktop_lifecycle::activate(&app);
                } else if let Some(attachment) = attachment.filter(|_| is_account_callback(&url)) {
                    if attachment
                        .post_account_callback(url.as_str())
                        .await
                        .is_err()
                    {
                        log::debug!("[account] callback forwarding failed");
                    }
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

fn is_account_callback(url: &url::Url) -> bool {
    url.scheme() == "domios" && url.host_str() == Some("auth") && url.path() == "/callback"
}

fn is_focus_action(url: &url::Url) -> bool {
    url.as_str() == "domios://focus"
}

fn billing_return_status(url: &url::Url) -> Option<Option<String>> {
    if url.scheme() != "domios"
        || url.host_str() != Some("billing")
        || url.path() != "/return"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let mut status = None;
    for (key, value) in url.query_pairs() {
        if key != "status"
            || status.is_some()
            || value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return None;
        }
        status = Some(value.into_owned());
    }
    Some(status)
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
        && identifier == "com.domios.app"
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn registration_needs_repair(production: bool, matches_executable: bool) -> bool {
    production && !matches_executable
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests;

#[cfg(target_os = "linux")]
fn linux_registration_matches(
    app: &codeterm_client_shell::app_runtime::AppHandle,
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
