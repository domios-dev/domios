#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let resolved = if path.starts_with("~/") || path.starts_with("~\\") {
        dirs::home_dir()
            .ok_or("Cannot find home directory")?
            .join(&path[2..])
    } else {
        std::path::PathBuf::from(&path)
    };
    if resolved.exists() {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(&resolved)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&resolved).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&resolved)
                .spawn();
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlApplication {
    Tailscale,
}

#[tauri::command]
pub async fn open_url(url: String, application: Option<UrlApplication>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if matches!(application, Some(UrlApplication::Tailscale)) && open_tailscale() {
            return Ok(());
        }
        codeterm_util::open_url::open_url(url)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn open_tailscale() -> bool {
    #[cfg(target_os = "macos")]
    return std::process::Command::new("open")
        .args(["-a", "Tailscale"])
        .status()
        .is_ok_and(|status| status.success());
    #[cfg(target_os = "windows")]
    return ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .flat_map(|root| {
            ["Tailscale", "Tailscale IPN"].map(|dir| root.join(dir).join("tailscale-ipn.exe"))
        })
        .any(|program| program.is_file() && std::process::Command::new(program).spawn().is_ok());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

#[tauri::command]
pub async fn get_autostart_enabled(app: crate::app_runtime::AppHandle) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || crate::autostart::is_enabled(&app))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_autostart_enabled(
    app: crate::app_runtime::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if enabled {
            crate::autostart::enable(&app)
        } else {
            crate::autostart::disable(&app)
        }
    })
    .await
    .map_err(|error| error.to_string())?
}
