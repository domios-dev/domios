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
        { let _ = std::process::Command::new("explorer").arg(&resolved).spawn(); }
        #[cfg(target_os = "macos")]
        { let _ = std::process::Command::new("open").arg(&resolved).spawn(); }
        #[cfg(target_os = "linux")]
        { let _ = std::process::Command::new("xdg-open").arg(&resolved).spawn(); }
    }
    Ok(())
}

#[tauri::command]
pub fn test_notification(_app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args(["-e", "display notification \"Notifications are working!\" with title \"CodeTerm\""])
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = _app.notification().builder()
            .title("CodeTerm")
            .body("Notifications are working!")
            .show();
    }
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
    { let _ = std::process::Command::new("open").arg(&url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
    Ok(())
}

#[tauri::command]
pub fn get_autostart_enabled() -> bool {
    crate::autostart::is_enabled()
}

#[tauri::command]
pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    if enabled { crate::autostart::enable() } else { crate::autostart::disable() }
}
