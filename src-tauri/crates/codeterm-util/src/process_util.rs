//! Cross-cutting `Command`-spawning helpers. The daemon is a windowed Tauri app,
//! so a console subprocess flashes a `conhost.exe` window on Windows by default;
//! route every spawn through here to suppress the flicker.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(all(windows, debug_assertions))]
pub mod diagnostics;

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

/// Same idea as `no_window_std` for `tokio::process::Command`. tokio exposes
/// `creation_flags` as an inherent method on Windows, so no trait
/// import is needed here.
#[cfg(windows)]
pub fn no_window_tokio(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
pub fn no_window_tokio_suspended(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW | 0x0000_0004);
}

#[cfg(not(windows))]
#[inline]
pub fn no_window_tokio(_cmd: &mut tokio::process::Command) {}

mod capture;
pub use capture::{bounded_capture, CaptureExit};
