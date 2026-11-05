//! Unified provider surface — one registry fronting PTY agents and chat backends.

mod catalogue_cache;
mod profile_readiness;
pub(crate) use profile_readiness::invalidate as invalidate_profile_readiness;
#[cfg(test)]
pub(crate) use profile_readiness::revision as profile_readiness_revision;
mod registry;
pub(crate) mod values;
pub use values::list_agent_provider_metadata;
pub use values::list_agent_provider_values as values_list_agent_provider_values;

pub use registry::ProviderRegistry;
