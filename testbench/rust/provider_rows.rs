//! Corpus-side resolution of a provider's status-bar band.
//!
//! The state dumps are organised by provider, so a replay can ask that
//! provider's plugin manifest for the same band the live path excludes. An
//! unresolvable provider directory is an error here too — never a zero.

use std::path::PathBuf;

use codeterm_lib::agent::classify_input::{StatusBarRows, StatusBarRowsError, StatusBarRowsSource};
use codeterm_lib::agent::provider::AgentProvider;
use codeterm_lib::plugin::js_runtime::load_one;

pub struct PluginManifestRows;

impl StatusBarRowsSource for PluginManifestRows {
    fn status_bar_rows_for(&self, agent_type: &str) -> Option<u16> {
        let manifest = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
            .join(format!("plugins/provider-{agent_type}/plugin.json"));
        load_one(&manifest).ok().map(|p| p.status_bar_rows())
    }
}

pub fn status_bar_rows(provider: &str) -> Result<StatusBarRows, StatusBarRowsError> {
    StatusBarRows::resolve(&PluginManifestRows, provider)
}
