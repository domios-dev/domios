//! Linux orchestrator — placeholder.
//!
//! Same architecture as Windows — the dispatch mechanism (`at` if
//! available, otherwise fork+detach+setsid) needs its own design pass.

use std::path::PathBuf;

use super::{Orchestrator, OrchestratorInputs};

pub struct LinuxOrchestrator;

impl Orchestrator for LinuxOrchestrator {
    fn render_script(&self, _inputs: &OrchestratorInputs) -> String {
        String::new()
    }

    fn dispatch(&self, _inputs: &OrchestratorInputs) -> Result<PathBuf, String> {
        Err(unsupported_message())
    }
}

pub(crate) fn unsupported_message() -> String {
    "Linux self-update orchestrator not yet implemented; falling back to `codeterm update install --auto`".to_string()
}
