//! ChatPreprocessor capability — send-time `text` slot of the outbound pipeline.
//! Trait + context types live in [`crate::bridge::ports`]; registry is bridge-owned.

use std::sync::Arc;

pub use crate::bridge::ports::{ChatPreprocessorProvider, PreprocessCtx};
pub use crate::bridge::registry::CapabilityRegistry;

pub type ChatPreprocessorRegistry = CapabilityRegistry<dyn ChatPreprocessorProvider>;
pub type ChatPreprocessorRegistryHandle = Arc<ChatPreprocessorRegistry>;
