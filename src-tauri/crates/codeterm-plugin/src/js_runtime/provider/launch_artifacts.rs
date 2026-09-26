use crate::provider::{LaunchArtifactSweepReport, LaunchArtifactSweepRequest};
use serde_json::json;

use super::JsProvider;

pub(super) fn release(provider: &JsProvider, tab_id: &str, instance_id: &str) -> Result<bool, String> {
    const METHOD: &str = "releaseTabLaunchArtifacts";
    if provider.has_js(METHOD) {
        return super::call_json_strict(
            &provider.vm,
            METHOD,
            &[json!(tab_id), json!(instance_id)],
        )
        .map_err(|error| format!("[plugin {}] {METHOD} failed: {error}", provider.manifest.id));
    }
    provider
        .native()
        .map(|native| native.release_tab_launch_artifacts(tab_id, instance_id))
        .unwrap_or(Ok(false))
}

pub(super) fn sweep(
    provider: &JsProvider,
    request: &LaunchArtifactSweepRequest,
) -> Result<Option<LaunchArtifactSweepReport>, String> {
    const METHOD: &str = "sweepLaunchArtifacts";
    if provider.has_js(METHOD) {
        let report = super::call_json_strict(&provider.vm, METHOD, &[json!(request)])
            .map_err(|error| format!("[plugin {}] {METHOD} failed: {error}", provider.manifest.id))?;
        return Ok(Some(report));
    }
    provider
        .native()
        .map(|native| native.sweep_launch_artifacts(request))
        .unwrap_or(Ok(None))
}
