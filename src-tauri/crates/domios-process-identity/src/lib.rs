#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "process_unix.rs"]
mod platform;

pub use platform::PinnedProcess;

pub mod client_identity;
#[cfg(target_os = "macos")]
pub mod code_signature_macos;

extern crate domios_types as codeterm_types; // rename-bridge
