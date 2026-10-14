//! Contracts the core calls into when the implementation lives in a crate above it.
//!
//! Rules every port keeps:
//! - Methods take and return plain data or `Arc` registry handles, never the service
//!   context, multiplexer state, a lock or a closure that captures one.
//! - This module imports nothing from the PTY crate, the multiplexer or the lock wrappers.
//! - Methods are synchronous; an async caller wraps the call in `spawn_blocking`.
//! - Implementations may lock; callers hold no ordered lock while calling a port.
//! - Every trait is object-safe and `Send + Sync`, asserted with `assert_port`.
//! - Doc comments on ports stay within two lines.

pub mod rest;
pub mod socket;
pub mod sentinel;
pub mod transport;
pub mod update;

// Later lanes add one `assert_port::<dyn Trait>()` line per port trait.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn assert_port<T: ?Sized + Send + Sync>() {}
