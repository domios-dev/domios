//! Plugin surface tabs: a core-owned tab frame whose content a plugin view renders.

mod activity;
mod agent_call;
mod exit;
mod host;
mod model;
mod ops;
pub(crate) mod resume;
pub(crate) mod runtime;
mod step_call;
pub use runtime::sse_payload as runtime_sse_payload;
mod stream_ops;
mod transport;
mod view;
mod view_context;

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
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{Tab, TabBacking, TabCloseReason};
use codeterm_browser::BrowserStatus;

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
    pub fn start_browser_orphan_sweep(&self) {
        let runtimes = self.ext::<std::sync::Arc<SurfaceRuntimeRegistry>>().clone();
        crate::async_runtime::spawn(async move { runtimes.sweep_browsers().await });
    }

    pub fn tab_surface_input_refusal(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        let resolved = mux.resolve_tab_id(tab_id)?;
        surface_input_refusal(&mux.get_tab(&resolved)?.backing)
    }

    fn is_browser_surface(&self, tab: &Tab) -> bool {
        let Some(backing) = tab.surface() else {
            return false;
        };
        self.ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
            .get(&backing.plugin_id)
            .is_some_and(|declared| {
                declared.iter().any(|decl| {
                    decl.id == backing.surface_id
                        && decl.stream_runtime() == Some(codeterm_surface::surface::BROWSER_RUNTIME)
                })
            })
    }

    pub(crate) fn browser_tab_status(&self, tab: &Tab) -> Option<BrowserStatus> {
        self.is_browser_surface(tab).then(|| {
            self.ext::<std::sync::Arc<SurfaceRuntimeRegistry>>()
                .browser_status(&tab.id)
        })
    }

    pub(crate) fn browser_tab_status_fields(
        &self,
        tab: &Tab,
    ) -> Option<(&'static str, Option<String>)> {
        Some(match self.browser_tab_status(tab)? {
            BrowserStatus::Closed => ("closed", None),
            BrowserStatus::Running => ("running", None),
            BrowserStatus::Error(reason) => ("error", Some(reason)),
        })
    }

    pub(crate) fn remove_owned_browser_tabs(
        &self,
        mux: &mut MultiplexerState,
        owner: &str,
    ) -> bool {
        let mut removed = false;
        for id in mux.get_children_ids(owner) {
            if mux
                .get_tab(&id)
                .is_some_and(|tab| self.is_browser_surface(tab))
            {
                match mux.close_tab_with_reason(&id, TabCloseReason::AgentRunEnded) {
                    Ok(_) => removed = true,
                    Err(error) => log::debug!("[browser] owner exit close {id}: {error}"),
                }
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests;
