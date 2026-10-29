//! `view` capability — a plugin ships an HTML/JS/CSS UI rendered in a sandboxed,
//! CSP-locked iframe that reaches the host only through the `ct.invoke` bridge.
//! Security boundary: the bridge routes to the single `viewCall` export plus
//! narrowly capability-gated host-owned settings, never arbitrary exports.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

mod context;
pub use context::{
    current_view_call_context, has_view_call_context, with_view_call_context, ViewActor,
    ViewCallContext, ViewCallOrigin, ViewOwnerScope,
};

use crate::capability::CapabilityRegistry;
use crate::js_runtime::{ViewHostAction, WebhookReceiverKind};

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

/// A plugin iframe backend. Host-owned ports remain separate from viewCall.
#[async_trait]
pub trait ViewProvider: Send + Sync {
    fn invocation_runtime(&self) -> Option<String> {
        None
    }
    fn id(&self) -> &str;

    fn job_lifecycle(&self) -> Option<crate::plugin_jobs::JobLifecycleDescriptor> {
        None
    }
    fn reconcile_job(
        &self,
        _call: &crate::plugin_jobs::JobCall,
    ) -> Result<crate::plugin_jobs::JobObservation, ViewError> {
        Err(ViewError::Call("job lifecycle unavailable".into()))
    }
    fn cancel_job(
        &self,
        _call: &crate::plugin_jobs::JobCall,
    ) -> Result<crate::plugin_jobs::JobObservation, ViewError> {
        Err(ViewError::Call("job cancellation unavailable".into()))
    }

    /// The webhook capability served by this view, when it has one.
    fn webhook_receiver_kind(&self) -> Option<WebhookReceiverKind> {
        None
    }

    /// The user-gesture host actions the plugin declared for its view; nothing else is performed.
    fn host_actions(&self) -> &[ViewHostAction] {
        &[]
    }

    fn view_inputs(&self) -> Option<crate::media_delivery::ViewInputDescriptor> {
        None
    }

    fn media_delivery(&self) -> Option<crate::media_delivery::MediaDeliveryDescriptor> {
        None
    }

    async fn media_file(
        &self,
        _id: &str,
        _context: &crate::view_host::ViewCallContext,
    ) -> Result<crate::media_delivery::MediaFile, ViewError> {
        Err(ViewError::Call("media delivery not supported".into()))
    }

    async fn run_media_file(
        &self,
        _id: &str,
        _context: &crate::media_delivery::RunMediaLookupContext,
    ) -> Result<crate::media_delivery::MediaFile, ViewError> {
        Err(ViewError::Call("durable media lookup not supported".into()))
    }

    /// Dispatch one `ct.invoke(method, args)` from the iframe to the plugin's
    /// `viewCall(method, args)` export. Returns its JSON result.
    async fn call(
        &self,
        method: &str,
        args: Value,
        context: &ViewCallContext,
    ) -> Result<Value, ViewError>;
}

pub type ViewRegistry = CapabilityRegistry<dyn ViewProvider>;
pub type ViewRegistryHandle = Arc<ViewRegistry>;

/// Resolve a plugin's on-disk directory by id, scanning the same roots the
/// snapshot uses. The iframe asset route serves `<dir>/ui/*` from here.
pub fn plugin_ui_dir(plugin_id: &str) -> Option<std::path::PathBuf> {
    let workspace = workspace_root();
    for root in crate::js_runtime::default_roots(Some(workspace.as_path())) {
        let dir = root.join(plugin_id);
        if dir.join("ui").join("index.html").exists() {
            return Some(dir.join("ui"));
        }
    }
    None
}

// This crate sits at src-tauri/crates/codeterm-plugin; the repository root owns `plugins/`.
fn workspace_root() -> std::path::PathBuf {
    #[cfg(test)]
    let root = crate::test_support::fixture_manifest_dir().join("../../..");
    #[cfg(not(test))]
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."));
    root
}

#[cfg(test)]
mod tests {
    #[test]
    fn workspace_root_resolves_the_in_repo_view_plugin_ui() {
        let ui = super::workspace_root()
            .join("plugins")
            .join("analytics")
            .join("ui")
            .join("index.html");
        assert!(ui.exists(), "missing {}", ui.display());
    }
}
