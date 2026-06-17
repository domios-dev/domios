//! ChatPreprocessor capability — the send-time text stage of the outbound
//! message pipeline (plan §6). A plugin matches the *final text* of an outbound
//! turn and augments it (mem recall, a worker, Groq), or passes it through. It
//! is the more general sibling of the transcriber (which grabs the `voice` slot
//! at compose-time); the preprocessor grabs the `text` slot at send-time.
//!
//! There is no native preprocessor — the capability is JS-only (plan §1.10), so
//! the acceptance oracle is a `call_json` contract test (see
//! `plugin::js_runtime::chatpreprocessor`), not `parity_test.rs`.
//!
//! Core asks the capability, never the identity — resolution goes through
//! [`ChatPreprocessorRegistry`] (a
//! [`CapabilityRegistry`](crate::plugin::capability::CapabilityRegistry)). The
//! ordered run + conflict guard live in [`crate::chatpipeline`].

use std::sync::Arc;

use crate::plugin::capability::CapabilityRegistry;

/// Context handed to a preprocessor for one outbound turn. Mirrors the SDK
/// `chatPreprocess(ctx)` shape; `text` is passed separately to the trait method
/// and merged into the JS ctx by the adapter.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreprocessCtx {
    pub pane_id: String,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub turn_index: u64,
}

/// A send-time text stage. Implemented by JS plugins (`JsChatPreprocessor`).
///
/// `preprocess` returns `Some(text)` to **replace** the outbound text, or `None`
/// to **pass through** unchanged. A broken plugin (throw / bad output) must fail
/// safe to `None` — a preprocessor never drops the user's message.
pub trait ChatPreprocessorProvider: Send + Sync {
    fn id(&self) -> &str;
    fn preprocess(&self, ctx: &PreprocessCtx, text: &str) -> Option<String>;
}

pub type ChatPreprocessorRegistry = CapabilityRegistry<dyn ChatPreprocessorProvider>;
pub type ChatPreprocessorRegistryHandle = Arc<ChatPreprocessorRegistry>;
