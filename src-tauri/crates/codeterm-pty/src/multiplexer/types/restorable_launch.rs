use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TabAccount {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub namespace: Option<String>,
    pub notices: Vec<AccountContinuationNotice>,
}

pub(super) fn serialize_account<S: serde::Serializer>(
    launch: &RestorableLaunchParameters,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    TabAccount {
        id: launch
            .account_id
            .clone()
            .unwrap_or_else(|| "default".into()),
        namespace: launch.account_namespace.clone(),
        notices: launch.account_notices.clone(),
    }
    .serialize(serializer)
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_notices: Vec<AccountContinuationNotice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_namespace: Option<String>,
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
    #[ts(skip)]
    pub exited_chat_binding: Option<ExitedChatBinding>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    pub runtime_prompt_source: Option<RuntimePromptSource>,
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
        self.account_notices.is_empty()
            && self.account_id.is_none()
            && self.account_namespace.is_none()
            && self.execution_mode.is_none()
            && self.execution_sandbox.is_none()
            && self.failed_provider.is_none()
            && self.exited_chat_binding.is_none()
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
            && self.runtime_prompt_source.is_none()
            && !self.permission_bypass_requested
            && self.runtime_launch_options.is_none()
            && self.host_approval_choice.is_none()
            && self.selected_session_mode.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePromptSource {
    Regenerable(String),
    Preserved(String),
}

impl RuntimePromptSource {
    pub fn fingerprint(prompt: &str) -> String {
        use sha2::{Digest, Sha256};

        format!("{:x}", Sha256::digest(prompt.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitedChatBinding {
    pub provider_id: String,
    pub nonce: String,
    pub session_id: String,
}

impl super::Tab {
    pub fn exited_chat_recipient(&self) -> Option<&ExitedChatBinding> {
        if !matches!(self.backing, super::TabBacking::Pty) || self.agent_type.is_some() {
            return None;
        }
        self.restorable_launch.exited_chat_binding.as_ref()
    }

    pub fn exited_chat_resume_target(&self) -> Option<&ExitedChatBinding> {
        let exit = self.restorable_launch.exited_chat_binding.as_ref()?;
        let resume = self.prior_session_binding_nonces.last()?;
        (self.agent_type.as_ref() == Some(&exit.provider_id)
            && self.session_binding_nonce.as_ref() == Some(&resume.nonce)
            && resume.resume_target_session_id.as_ref() == Some(&exit.session_id)
            && self
                .agent_session_id
                .as_ref()
                .is_none_or(|session| session == &exit.session_id))
        .then_some(exit)
    }

    pub fn resuming_exited_chat_recipient(&self) -> Option<&ExitedChatBinding> {
        self.exited_chat_resume_target().filter(|_| {
            !self.launch_session_authenticated()
                && self.boot_stage != Some(codeterm_types::pty::BootStage::SpawnFailed)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountContinuationNotice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<String>,
    pub id: String,
    pub provider: String,
    pub session: String,
    pub at_ms: u64,
    pub content: String,
    pub boundary: Option<String>,
    pub resets_at_ms: Option<u64>,
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
