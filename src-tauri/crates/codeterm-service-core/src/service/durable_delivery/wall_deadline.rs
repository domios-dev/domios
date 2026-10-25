//! One-shot absolute wall-clock failure deadlines; cancellation never delivers a payload.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(super) use linux::WallDeadline;
#[cfg(target_os = "macos")]
pub(super) use macos::WallDeadline;
#[cfg(windows)]
pub(super) use windows::WallDeadline;

#[cfg(test)]
mod tests;
