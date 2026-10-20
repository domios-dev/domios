//! ChatPreprocessor capability — send-time `text` slot of the outbound pipeline.

use std::sync::Arc;

pub use codeterm_bridge::ports::{ChatPreprocessorProvider, PreprocessCtx};
pub use codeterm_bridge::registry::CapabilityRegistry;

pub type ChatPreprocessorRegistry = CapabilityRegistry<dyn ChatPreprocessorProvider>;
pub type ChatPreprocessorRegistryHandle = Arc<ChatPreprocessorRegistry>;
