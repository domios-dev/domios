//! `view` capability — a plugin ships a full HTML/JS/CSS UI (`ui/index.html`)
//! rendered in a **sandboxed iframe** (VS Code / Figma webview model). The iframe
//! is origin-`null`, CSP-locked (`connect-src 'none'`), and can reach the host
//! ONLY through the `ct.invoke` postMessage bridge, which lands here.
//!
//! Security boundary: the bridge routes to a SINGLE plugin export — `viewCall` —
//! never arbitrary exports, so an XSS'd iframe can't reach capability methods
//! (e.g. a secret backend's `secretGetItem`). The plugin's `viewCall` dispatches
//! only the UI-safe operations it chooses to expose; `host.*` calls inside it stay
//! permission-gated by the VM sandbox.

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
    // Same roots the snapshot scans: data-dir installs + the workspace `plugins/`
    // in dev (so workspace-loaded plugins serve their UI too).
    let workspace = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    for root in crate::plugin::js_runtime::default_roots(Some(workspace.as_path())) {
        let dir = root.join(plugin_id);
        if dir.join("ui").join("index.html").exists() {
            return Some(dir.join("ui"));
        }
    }
    None
}
