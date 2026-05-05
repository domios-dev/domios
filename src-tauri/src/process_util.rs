//! Cross-cutting helpers around `Command` spawning.
//!
//! The CodeTerm daemon is a GUI (windowed-subsystem) Tauri app, so any
//! console subprocess we spawn — `bw`, `tailscale`, `powershell`, `reg`,
//! `cmd /c …`, embed/rerank binaries — flashes a fresh `conhost.exe`
//! window on Windows by default. Pass every spawn through here and the
//! flicker disappears.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Suppress the auto-spawned console window for `std::process::Command`
/// on Windows. No-op everywhere else.
#[cfg(windows)]
pub fn no_window_std(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
pub fn no_window_std(_cmd: &mut std::process::Command) {}

/// Same idea for `tokio::process::Command`. tokio exposes
/// `creation_flags` as an inherent method on Windows, so no trait
/// import is needed here.
#[cfg(windows)]
pub fn no_window_tokio(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
pub fn no_window_tokio(_cmd: &mut tokio::process::Command) {}
