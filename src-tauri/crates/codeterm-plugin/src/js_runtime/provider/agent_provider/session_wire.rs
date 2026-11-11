use serde::Deserialize;
use serde_json::Value;

use super::super::JsProvider;
use crate::provider::AgentProvider;

/// `usageSources` wire row: a path plus the session that scopes a shared log.
#[derive(Deserialize, ts_rs::TS)]
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

pub(super) fn export_protocol_contracts(
    config: &ts_rs::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    WireUsageSource::export_all(config)?;
    Ok(())
}
