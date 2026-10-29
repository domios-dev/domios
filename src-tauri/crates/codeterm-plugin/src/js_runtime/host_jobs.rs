use super::{host_api::PluginSandbox, vm::VmError};
use std::sync::Arc;

pub(super) fn install(
    host: &rquickjs::Object<'_>,
    sandbox: &Arc<PluginSandbox>,
) -> Result<(), VmError> {
    let sandbox = sandbox.clone();
    host.set(
        "pluginJobInvoke",
        rquickjs::function::Func::new(move |request: String| {
            if request.len() > 4096
                || !codeterm_decision::physical::effects_allowed()
                || crate::media_delivery::run_media_lookup_active()
            {
                return r#"{"error":"plugin_job_request_refused"}"#.to_string();
            }
            sandbox.service().map_or_else(
                || r#"{"error":"plugin_jobs_unavailable"}"#.into(),
                |service| service.plugin_job_json(sandbox.plugin_id(), &request),
            )
        }),
    )
    .map_err(|_| VmError::Script("set pluginJobInvoke failed".into()))
}
