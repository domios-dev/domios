#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "process_unix.rs"]
mod platform;

pub use platform::PinnedProcess;

#[cfg(test)]
mod artifact_digest;
pub mod client_identity;
#[cfg(target_os = "macos")]
pub mod code_signature_macos;
