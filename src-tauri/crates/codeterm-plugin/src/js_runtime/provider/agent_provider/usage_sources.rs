use super::session_wire::WireUsageSource;
use super::{provider, JsProvider, UsageSourceSpec};
use serde_json::json;
use std::path::PathBuf;

impl JsProvider {
    pub(super) fn js_usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        if self.has_js("usageSources") {
            // An exported `usageSources` is authoritative: a null or failed reply
            // declares no source and must not fall back to a fabricated file.
            return self
                .try_call::<Vec<WireUsageSource>>("usageSources", &[json!(cwd), json!(session_id)])
                .unwrap_or_default()
                .into_iter()
                .filter(|source| !source.path.is_empty())
                .map(|source| UsageSourceSpec {
                    path: PathBuf::from(source.path),
                    session_id: source.session_id.filter(|id| !id.is_empty()),
                })
                .collect();
        }
        provider::usage_sources_from_session_file(self, cwd, session_id)
    }
}
