//! Unified provider surface — one registry fronting PTY agents and chat backends.

mod catalogue_cache;
mod registry;
pub(crate) mod values;
pub use values::list_agent_provider_values as values_list_agent_provider_values;

pub use registry::ProviderRegistry;
