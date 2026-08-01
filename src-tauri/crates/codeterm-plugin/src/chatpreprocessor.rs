//! Chat send-time text preprocessing capability.

use std::sync::Arc;

use crate::capability::CapabilityRegistry;

/// Send-time text stage context (mirrors SDK `chatPreprocess(ctx)`).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreprocessCtx {
    pub tab_id: String,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub turn_index: u64,
}

/// A send-time text stage. Returns `Some(text)` to replace or `None` to pass through.
pub trait ChatPreprocessorProvider: Send + Sync {
    fn id(&self) -> &str;
    fn preprocess(&self, ctx: &PreprocessCtx, text: &str) -> Option<String>;
}

pub trait ChatPreprocessorPort: Send + Sync {
    fn preprocessor_ids(&self) -> Vec<String>;
    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>>;
}

pub type ChatPreprocessorRegistry = CapabilityRegistry<dyn ChatPreprocessorProvider>;
pub type ChatPreprocessorRegistryHandle = Arc<ChatPreprocessorRegistry>;

impl ChatPreprocessorPort for CapabilityRegistry<dyn ChatPreprocessorProvider> {
    fn preprocessor_ids(&self) -> Vec<String> {
        self.ids()
    }

    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>> {
        self.active()
    }
}

impl ChatPreprocessorPort for Arc<CapabilityRegistry<dyn ChatPreprocessorProvider>> {
    fn preprocessor_ids(&self) -> Vec<String> {
        self.ids()
    }

    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>> {
        self.active()
    }
}
