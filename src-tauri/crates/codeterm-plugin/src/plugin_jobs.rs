use crate::{
    blueprint_step::{OperationState, PluginJobOrigin, PluginJobScope},
    converter::ConversionCancellation,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobLifecycleDescriptor {
    pub contract_version: u16,
    pub reconcile: bool,
    pub cancel: bool,
}
impl JobLifecycleDescriptor {
    pub fn valid(&self) -> bool {
        self.contract_version == 1 && self.reconcile
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobContext {
    pub scope: PluginJobScope,
    pub origin: PluginJobOrigin,
    pub operation: String,
    pub job: String,
}

pub struct JobCall {
    pub context: JobContext,
    pub deadline: Instant,
    pub cancellation: ConversionCancellation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobObservation {
    pub job: String,
    pub state: OperationState,
}

impl JobObservation {
    pub fn valid_for(&self, job: &str) -> bool {
        self.job == job
            && matches!(
                self.state,
                OperationState::Queued
                    | OperationState::Running
                    | OperationState::Succeeded
                    | OperationState::Failed
                    | OperationState::Canceled
                    | OperationState::Unknown
            )
    }
}

pub trait PluginJobAdmission: Send + Sync {
    fn invoke(&self, plugin: &str, request: &str) -> Result<serde_json::Value, String>;
}

pub struct PluginJobs(pub std::sync::Arc<dyn PluginJobAdmission>);
