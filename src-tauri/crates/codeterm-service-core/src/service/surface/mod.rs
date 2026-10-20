//! Plugin surface tabs: a core-owned tab frame whose content a plugin view renders.

mod activity;
mod agent_call;
mod exit;
mod host;
mod model;
mod ops;
pub(crate) mod resume;
pub(crate) mod runtime;
pub use runtime::sse_payload as runtime_sse_payload;
mod stream_ops;
mod transport;

pub(crate) use activity::{is_agent_method, SurfaceActivityGate};
pub use exit::{begin_surface_shutdown, SurfaceShutdown};
pub use model::SurfaceError;
pub use model::SurfaceOpenRequest;
pub(crate) use runtime::SurfaceRuntimeRegistry;
pub use stream_ops::call_surface;
pub use stream_ops::open_surface_stream;
pub use stream_ops::CallOrigin;
pub use stream_ops::SurfaceCallFailure;
pub(crate) use transport::SurfaceTransport;

use super::ServiceContext;
use crate::multiplexer::types::TabBacking;

pub(crate) const SURFACE_NO_INPUT: &str = "surface_no_input";
pub(crate) const SURFACE_NO_TEXT: &str = "surface_no_text";

/// The typed refusal every text path returns for a surface tab; `None` for any other backing.
pub(crate) fn surface_input_refusal(backing: &TabBacking) -> Option<String> {
    matches!(backing, TabBacking::Surface(_)).then(transport::no_input)
}

pub(crate) fn builtin_runtimes() -> std::sync::Arc<SurfaceRuntimeRegistry> {
    std::sync::Arc::new(SurfaceRuntimeRegistry::with_builtin_runtimes(
        &crate::paths::data_dir(),
    ))
}

impl ServiceContext {
    pub fn tab_surface_input_refusal(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        let resolved = mux.resolve_tab_id(tab_id)?;
        surface_input_refusal(&mux.get_tab(&resolved)?.backing)
    }
}

#[cfg(test)]
mod tests;
