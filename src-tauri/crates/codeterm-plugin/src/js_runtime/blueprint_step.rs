use super::vm::{call_json_interruptible, PluginVm};
use crate::blueprint_step::{BlueprintStep, Estimate, JobUpdate, StepCall};
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
}

impl BlueprintStep for JsBlueprintStep {
    fn estimate(&self, call: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        self.call("blueprintEstimate", call)
    }
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.call("blueprintSubmit", call)
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.call("blueprintReconcile", call)
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.call("blueprintCancel", call)
    }
}
