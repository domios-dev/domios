//! Point the PTY fork at the bundled windowless ConPTY host (conpty.dll +
//! OpenConsole.exe) so spawning tabs doesn't flash an in-box conhost window.

/// Export CODETERM_CONPTY_DIR for this arch's bundled host. Must run before the
/// first tab/PTY spawn (the fork reads it on first CreatePseudoConsole).
#[cfg(windows)]
pub(crate) fn setup_conpty_host(app: &tauri::App) {
    use tauri::Manager;
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let Ok(dir) = app.path().resolve(
        format!("conpty/{arch}"),
        tauri::path::BaseDirectory::Resource,
    ) else {
        return;
    };
    if dir.join("conpty.dll").exists() {
        std::env::set_var("CODETERM_CONPTY_DIR", &dir);
        log::info!("[conpty] bundled host dir: {}", dir.display());
        crate::update::conpty_cleanup::reap_orphaned_hosts_in_background();
    } else {
        log::warn!("[conpty] bundled host missing at {}", dir.display());
    }
}

#[cfg(not(windows))]
pub(crate) fn setup_conpty_host(_app: &tauri::App) {}
