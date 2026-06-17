//! Status-bubble capability — a plugin contributes a small status pill into the
//! pane-footer bubble row (alongside the native Usage/Git bubbles). The pill is
//! pane-scoped: `bubble(ctx)` receives `{ cwd?, paneId?, provider? }` and returns
//! a [`BubbleData`] or `None` to hide for that context. Clicking the pill opens
//! the plugin's glance popover.
//!
//! Core asks the capability, never the identity — resolution goes through
//! [`BubbleRegistry`] (a [`CapabilityRegistry`](crate::plugin::capability::CapabilityRegistry)).

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin::capability::CapabilityRegistry;
use crate::view::Tone;

#[derive(Debug)]
pub enum BubbleError {
    Render(String),
}

impl std::fmt::Display for BubbleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(e) => write!(f, "bubble render failed: {e}"),
        }
    }
}

impl std::error::Error for BubbleError {}

/// A status pill. `tone` reuses the shared [`Tone`] token set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BubbleData {
    pub label: String,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Click behaviour. Reserved ids (`openPanel:<id>`, `settings`) open a host
    /// view directly; absent ⇒ the bubble opens the plugin's glance popover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// A source of pane-footer status pills. Implemented by `JsBubbleProvider`.
#[async_trait]
pub trait BubbleProvider: Send + Sync {
    fn id(&self) -> &str;

    /// Compute the pill for a pane context. `None` hides it for that pane.
    async fn bubble(&self, ctx: Value) -> Result<Option<BubbleData>, BubbleError>;
}

pub type BubbleRegistry = CapabilityRegistry<dyn BubbleProvider>;
pub type BubbleRegistryHandle = Arc<BubbleRegistry>;
