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

/// What the OS says about desktop toasts right now, without raising a prompt.
#[tauri::command]
pub async fn notify_permission_state(app: codeterm_service_core::app_runtime::AppHandle) -> String {
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::notification::os_notify::toast_permission(&app).to_string()
    })
    .await
    .unwrap_or_else(|_| "unknown".to_string())
}

/// The enabling gesture: switching the surface on asks the OS, switching it off
/// asks nothing and just reports what is already known. Off the runtime, because
/// a real OS prompt waits on the user and would otherwise starve every tab.
#[tauri::command]
pub async fn establish_notify_permission(
    app: codeterm_service_core::app_runtime::AppHandle,
    enabling: bool,
) -> String {
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::notification::os_notify::establish_toast_permission(&app, enabling)
            .to_string()
    })
    .await
    .unwrap_or_else(|_| "unknown".to_string())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    codeterm_service_core::service::open_url_open_url(url)
}

#[tauri::command]
pub async fn get_autostart_enabled() -> Result<bool, String> {
    tokio::task::spawn_blocking(codeterm_service_core::autostart_is_enabled)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if enabled {
            codeterm_service_core::enable_autostart()
        } else {
            codeterm_service_core::disable_autostart()
        }
    })
    .await
    .map_err(|error| error.to_string())?
}
