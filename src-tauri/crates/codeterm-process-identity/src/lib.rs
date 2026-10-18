#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "process_unix.rs"]
mod platform;

pub use platform::PinnedProcess;
