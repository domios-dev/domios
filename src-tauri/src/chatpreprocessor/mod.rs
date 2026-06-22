//! ChatPreprocessor capability — the send-time `text` slot of the outbound
//! pipeline: a plugin augments the final outbound text or passes it through.
//! JS-only (acceptance oracle is a `call_json` contract test), resolved via
//! [`ChatPreprocessorRegistry`]; the ordered run + conflict guard live in [`crate::chatpipeline`].

use std::sync::Arc;

use crate::plugin::capability::CapabilityRegistry;

/// Context for one outbound turn (mirrors the SDK `chatPreprocess(ctx)` shape); `text` is passed separately.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreprocessCtx {
    pub pane_id: String,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub turn_index: u64,
}

/// A send-time text stage. `preprocess` returns `Some(text)` to replace or `None`
/// to pass through; a broken plugin fails safe to `None` (never drops the message).
pub trait ChatPreprocessorProvider: Send + Sync {
    fn id(&self) -> &str;
    fn preprocess(&self, ctx: &PreprocessCtx, text: &str) -> Option<String>;
}

pub type ChatPreprocessorRegistry = CapabilityRegistry<dyn ChatPreprocessorProvider>;
pub type ChatPreprocessorRegistryHandle = Arc<ChatPreprocessorRegistry>;
