//! macOS orchestrator — placeholder.
//!
//! The architecture mirrors the Windows path (kill / swap / start /
//! verify / rollback) but the dispatch mechanism and binary layout
//! (.app bundles vs flat .exe) need their own design pass.
//!
//! Until that lands, `--apply` falls back to the existing
//! `update install --auto` path which already handles macOS via
//! tauri-plugin-updater.

use std::path::PathBuf;

use super::{Orchestrator, OrchestratorInputs};

pub struct MacOsOrchestrator;

impl Orchestrator for MacOsOrchestrator {
    fn render_script(&self, _inputs: &OrchestratorInputs) -> String {
        String::new()
    }

    fn dispatch(&self, _inputs: &OrchestratorInputs) -> Result<PathBuf, String> {
        Err(unsupported_message())
    }
}

pub(crate) fn unsupported_message() -> String {
    "macOS self-update orchestrator not yet implemented; falling back to `codeterm update install --auto`".to_string()
}
