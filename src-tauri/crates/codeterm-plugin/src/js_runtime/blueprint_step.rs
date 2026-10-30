use super::vm::{call_json_interruptible, PluginVm};
use crate::blueprint_step::{valid_job_identity, BlueprintStep, JobUpdate, StepCall};
use std::sync::Arc;

pub struct JsBlueprintStep {
    vm: Arc<PluginVm>,
}

impl JsBlueprintStep {
    pub fn new(vm: Arc<PluginVm>) -> Self {
        Self { vm }
    }

    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        call: &StepCall<'_>,
    ) -> Result<T, String> {
        if call.cancellation.is_cancelled() || std::time::Instant::now() >= call.deadline {
            return Err("plugin step interrupted".into());
        }
        let submitting = method == "blueprintSubmit";
        let _input_scope =
            crate::blueprint_step::StepInputScope::enter(self.vm.plugin_id(), call, submitting);
        // The host-stamped run owner acts for the submit; the plugin can never name it.
        let _caller = submitting.then(|| {
            super::host_surface::AgentCallerScope::enter(&call.request.context.owner)
        });
        let mut value =
            serde_json::to_value(call.request).map_err(|_| "invalid plugin step request")?;
        value["job"] = serde_json::json!(call.job);
        value["deadlineMs"] = serde_json::json!(call
            .deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64);
        call_json_interruptible(
            &self.vm,
            method,
            &[value],
            call.cancellation.clone(),
            call.deadline,
        )
        .map_err(|_| "plugin step call failed".into())
    }

    fn job_call(&self, method: &str, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        let value: serde_json::Value = self.call(method, call)?;
        match serde_json::from_value::<JobUpdate>(value.clone()) {
            Ok(update) => Ok(update),
            Err(_) => value
                .get("job")
                .and_then(serde_json::Value::as_str)
                .filter(|job| valid_job_identity(job))
                .map(|job| JobUpdate::unknown(job.into()))
                .ok_or_else(|| "plugin step response invalid".into()),
        }
    }
}

impl BlueprintStep for JsBlueprintStep {
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call("blueprintSubmit", call)
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call("blueprintReconcile", call)
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call("blueprintCancel", call)
    }
}
