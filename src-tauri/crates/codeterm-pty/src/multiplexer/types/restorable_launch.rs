use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReasoningEffortIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub planned: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub requested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RestorableLaunchParameters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_sandbox: Option<codeterm_types::plan::ExecutionSandboxPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub failed_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub structured_runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context_files: Option<codeterm_types::plan::TaskFiles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context_skills: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worker_restart: Option<WorkerRestartState>,
    #[serde(default)]
    pub captured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub requested_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub identity_instructions: Option<String>,
    #[serde(default)]
    pub permission_bypass_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runtime_launch_options: Option<RestorableRuntimeLaunchOptions>,
    /// Absent means the owner never chose; a false launch record alone is not an owner choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host_approval_choice: Option<HostApprovalChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selected_session_mode: Option<String>,
}

/// The owner's per-tab answer to agent permission requests the host can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HostApprovalChoice {
    Ask,
    AllowAll,
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
            && self.host_approval_choice.is_none()
            && self.selected_session_mode.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RestorableRuntimeLaunchOptions {
    pub provider_id: String,
    pub binding_nonce: String,
    pub mode: String,
    pub additional_directories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub network_access: Option<bool>,
    /// The launch profile's declared approve-everything session mode; set only for full access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub full_access_mode: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkerRestartState {
    pub attempt_id: String,
    pub original_brief: String,
    pub sender_tab: String,
    #[serde(default)]
    pub pending: Vec<RestorableWorkerTurn>,
    #[serde(default)]
    pub claimed_nonce: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
