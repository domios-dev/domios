#[tauri::command]
pub fn frontend_ready(generation: u64) -> bool {
    crate::startup::mark_frontend_ready(generation)
}

#[tauri::command]
pub fn frontend_readiness_generation() -> u64 {
    crate::startup::frontend_readiness_generation()
}

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

#[tauri::command]
pub fn test_notification(_app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                "display notification \"Notifications are working!\" with title \"CodeTerm\"",
            ])
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = _app
            .notification()
            .builder()
            .title("CodeTerm")
            .body("Notifications are working!")
            .show();
    }
}

/// What the OS says about desktop toasts right now, without raising a prompt.
#[tauri::command]
pub async fn notify_permission_state(app: tauri::AppHandle) -> String {
    tokio::task::spawn_blocking(move || crate::api::os_notify::toast_permission(&app).to_string())
        .await
        .unwrap_or_else(|_| "unknown".to_string())
}

/// The enabling gesture: switching the surface on asks the OS, switching it off
/// asks nothing and just reports what is already known. Off the runtime, because
/// a real OS prompt waits on the user and would otherwise starve every tab.
#[tauri::command]
pub async fn establish_notify_permission(app: tauri::AppHandle, enabling: bool) -> String {
    tokio::task::spawn_blocking(move || {
        crate::api::os_notify::establish_toast_permission(&app, enabling).to_string()
    })
    .await
    .unwrap_or_else(|_| "unknown".to_string())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", &url]);
        crate::process_util::no_window_std(&mut cmd);
        let _ = cmd.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_autostart_enabled() -> Result<bool, String> {
    tokio::task::spawn_blocking(crate::autostart::is_enabled)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if enabled {
            crate::autostart::enable()
        } else {
            crate::autostart::disable()
        }
    })
    .await
    .map_err(|error| error.to_string())?
}
