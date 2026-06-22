//! `view` capability — a plugin ships an HTML/JS/CSS UI rendered in a sandboxed,
//! CSP-locked iframe that reaches the host only through the `ct.invoke` bridge.
//! Security boundary: the bridge routes to the single `viewCall` export (never
//! arbitrary ones), so an XSS'd iframe can't reach other capability methods.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::plugin::capability::CapabilityRegistry;

#[derive(Debug)]
pub enum ViewError {
    Call(String),
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call(e) => write!(f, "view call failed: {e}"),
        }
    }
}

impl std::error::Error for ViewError {}

/// A plugin that renders an iframe UI. `call` is the single bridge entry.
#[async_trait]
pub trait ViewProvider: Send + Sync {
    fn id(&self) -> &str;

    /// Dispatch one `ct.invoke(method, args)` from the iframe to the plugin's
    /// `viewCall(method, args)` export. Returns its JSON result.
    async fn call(&self, method: &str, args: Value) -> Result<Value, ViewError>;
}

pub type ViewRegistry = CapabilityRegistry<dyn ViewProvider>;
pub type ViewRegistryHandle = Arc<ViewRegistry>;

/// Resolve a plugin's on-disk directory by id, scanning the same roots the
/// snapshot uses. The iframe asset route serves `<dir>/ui/*` from here.
pub fn plugin_ui_dir(plugin_id: &str) -> Option<std::path::PathBuf> {
    // Same roots the snapshot scans: data-dir installs + the dev workspace `plugins/`.
    let workspace = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    for root in crate::plugin::js_runtime::default_roots(Some(workspace.as_path())) {
        let dir = root.join(plugin_id);
        if dir.join("ui").join("index.html").exists() {
            return Some(dir.join("ui"));
        }
    }
    None
}
