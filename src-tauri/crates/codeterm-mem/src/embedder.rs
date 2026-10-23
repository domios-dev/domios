// Providers are host adapters, never exposed to plugins, so no JS parity applies.

pub mod active;
pub mod catalog;
pub mod index;
pub mod key;
pub mod local;
mod manager;
pub mod openai;
pub mod pending;
pub mod provider;
mod shadow;
pub mod switch;

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;

use serde::Serialize;

pub use active::{ActiveEmbedder, EmbedderEvent, Observer};
pub use catalog::{BuildContext, BuildError, EmbedderSelection, ProviderDescriptor};
pub use provider::{Content, EmbedError, EmbedderIdentity, EmbeddingProvider, Modality};
pub use switch::{Clock, ReembedProgress, ReembedState, SwitchPlan, SystemClock};

#[derive(Debug, Clone, PartialEq)]
pub struct StampedVector {
    pub identity: EmbedderIdentity,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EmbedderStatus {
    pub provider: String,
    pub model: String,
    pub dimension: usize,
    pub remote: bool,
    pub available: bool,
    pub index: Option<EmbedderIdentity>,
    pub index_matches: bool,
    pub vectors: u64,
    pub pending: u64,
    pub degraded: bool,
    pub reembed: ReembedProgress,
}

pub fn legacy_identity() -> EmbedderIdentity {
    local::LocalProvider::new(local::default_model()).identity()
}
