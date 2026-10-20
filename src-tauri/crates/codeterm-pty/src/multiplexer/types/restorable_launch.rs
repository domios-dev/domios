use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningEffortIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_source: Option<String>,
}

impl ReasoningEffortIdentity {
    pub fn is_empty(&self) -> bool {
        self.planned.is_none()
            && self.requested.is_none()
            && self.resolved.is_none()
            && self.actual.is_none()
            && self.actual_source.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorableLaunchParameters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_sandbox: Option<codeterm_types::plan::ExecutionSandboxPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_files: Option<codeterm_types::plan::TaskFiles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_skills: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_restart: Option<WorkerRestartState>,
    #[serde(default)]
    pub captured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_instructions: Option<String>,
    #[serde(default)]
    pub permission_bypass_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_launch_options: Option<RestorableRuntimeLaunchOptions>,
}

impl RestorableLaunchParameters {
    pub fn restored_reasoning_effort_identity(
        &self,
        mut identity: ReasoningEffortIdentity,
    ) -> ReasoningEffortIdentity {
        if !self.is_empty() {
            identity.requested = self.requested_reasoning_effort.clone();
            identity.resolved = self.resolved_reasoning_effort.clone();
            identity.actual = None;
            identity.actual_source = (identity.requested.is_some() || identity.resolved.is_some())
                .then(|| "provider-default-unverified".to_string());
        }
        identity
    }

    pub fn is_empty(&self) -> bool {
        self.execution_mode.is_none()
            && self.execution_sandbox.is_none()
            && self.failed_provider.is_none()
            && self.structured_runtime_mode.is_none()
            && self.context_files.is_none()
            && self.context_skills.is_none()
            && self.worker_restart.is_none()
            && !self.captured
            && self.requested_model.is_none()
            && self.model.is_none()
            && self.requested_reasoning_effort.is_none()
            && self.resolved_reasoning_effort.is_none()
            && self.args.is_none()
            && self.agent_name.is_none()
            && self.identity_instructions.is_none()
            && !self.permission_bypass_requested
            && self.runtime_launch_options.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorableRuntimeLaunchOptions {
    pub provider_id: String,
    pub binding_nonce: String,
    pub mode: String,
    pub additional_directories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_access: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRestartState {
    pub attempt_id: String,
    pub original_brief: String,
    pub sender_tab: String,
    #[serde(default)]
    pub pending: Vec<RestorableWorkerTurn>,
    #[serde(default)]
    pub claimed_nonce: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorableWorkerTurn {
    pub delivery_id: String,
    pub sender_tab: String,
    pub provider_id: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub after_offset: u64,
    pub user_text: String,
    pub hops: u8,
}
