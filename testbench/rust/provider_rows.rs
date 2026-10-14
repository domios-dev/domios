//! Corpus-side resolution of a provider's status-bar band.
//!
//! The state dumps are organised by provider, so a replay can ask that
//! provider's plugin manifest for the same band the live path excludes. An
//! unresolvable provider directory is an error here too — never a zero.

use codeterm_service_core::agent::classify_input::{
    StatusBarRows, StatusBarRowsError, StatusBarRowsSource,
};
use codeterm_service_core::agent::provider::AgentProvider;
use codeterm_service_core::plugin::js_runtime::load_one;

pub struct PluginManifestRows;

impl StatusBarRowsSource for PluginManifestRows {
    fn status_bar_rows_for(&self, agent_type: &str) -> Option<u16> {
        let manifest = crate::test_support::fixture_manifest_dir()
            .join("..")
            .join(format!("plugins/provider-{agent_type}/plugin.json"));
        load_one(&manifest).ok().map(|p| p.status_bar_rows())
    }
}

pub fn status_bar_rows(provider: &str) -> Result<StatusBarRows, StatusBarRowsError> {
    StatusBarRows::resolve(&PluginManifestRows, provider)
}
