use serde::Deserialize;
use serde_json::Value;

use super::super::JsProvider;
use crate::provider::AgentProvider;

/// `usageSources` wire row: a path plus the session that scopes a shared log.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireUsageSource {
    pub(super) path: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

impl JsProvider {
    pub(super) fn detect_session_model_wire(&self, method: &str, args: &[Value]) -> Option<String> {
        self.try_call::<Option<String>>(method, args)
            .flatten()
            .map(|model| crate::model_catalog::resolve_observed_model(self.id(), model))
    }
}
