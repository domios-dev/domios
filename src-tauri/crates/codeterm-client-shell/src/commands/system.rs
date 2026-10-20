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
pub fn open_url(url: String) -> Result<(), String> {
    codeterm_util::open_url::open_url(url)
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
