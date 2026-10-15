//! Unified provider surface — one registry fronting PTY agents and chat backends.

mod registry;
pub(crate) mod values;

pub use registry::ProviderRegistry;
