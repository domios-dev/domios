//! Glance capability — a plugin's read-only popover surface (a [`GlanceView`]
//! node tree the host renders); buttons emit action ids via [`GlanceProvider::action`].
//! Core resolves via [`GlanceRegistry`], never an identity.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::plugin::capability::CapabilityRegistry;
use crate::view::GlanceView;

#[derive(Debug)]
pub enum GlanceError {
    Render(String),
}

impl std::fmt::Display for GlanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(e) => write!(f, "glance render failed: {e}"),
        }
    }
}

impl std::error::Error for GlanceError {}

/// A source of glance views. Implemented by `JsGlanceProvider` (drop-ins) and
/// any future native source.
#[async_trait]
pub trait GlanceProvider: Send + Sync {
    fn id(&self) -> &str;

    /// Render the current glance. `ctx` carries optional pane context
    /// (`{ cwd?, paneId?, provider? }`) — most plugins ignore it.
    async fn render(&self, ctx: Value) -> Result<GlanceView, GlanceError>;

    /// Handle a `Button` action id (anything not reserved by the host). Returns
    /// the refreshed view. Default: re-render.
    async fn action(&self, _action: &str, ctx: Value) -> Result<GlanceView, GlanceError> {
        self.render(ctx).await
    }
}

pub type GlanceRegistry = CapabilityRegistry<dyn GlanceProvider>;

/// Shared handle stored in app state.
pub type GlanceRegistryHandle = Arc<GlanceRegistry>;
