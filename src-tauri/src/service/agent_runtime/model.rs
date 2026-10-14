use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    ApprovalRequired,
    #[default]
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

pub(crate) struct ModelSwitchOptions {
    pub force: bool,
    pub restart_resume: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum RuntimeFailure {
    SpawnFailed {
        reason: String,
    },
    ProcessExited {
        code: Option<i32>,
        signal: Option<i32>,
        stderr: String,
        requested: bool,
    },
    PipeEof,
    PipeFailure {
        stream: String,
        reason: String,
    },
    InvalidFrame(String),
    Deadline,
    HookFailed(String),
    Refused(String),
    PrewriteRefused(String),
    Superseded,
}

impl std::fmt::Display for RuntimeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed { reason } => write!(formatter, "Runtime could not start: {reason}"),
            Self::ProcessExited {
                code,
                signal,
                stderr,
                requested,
            } => {
                formatter.write_str(if *requested {
                    "Runtime stopped"
                } else {
                    "Runtime exited"
                })?;
                if let Some(code) = code {
                    write!(formatter, " (code {code})")?;
                } else if let Some(signal) = signal {
                    write!(formatter, " (signal {signal})")?;
                }
                if !stderr.trim().is_empty() {
                    write!(formatter, ": {}", stderr.trim())?;
                }
                Ok(())
            }
            Self::PipeEof => formatter.write_str("Runtime connection closed"),
            Self::PipeFailure { stream, reason } => {
                write!(formatter, "Runtime {stream} connection failed: {reason}")
            }
            Self::InvalidFrame(reason) => {
                write!(formatter, "Runtime sent an invalid response: {reason}")
            }
            Self::Deadline => formatter.write_str("Runtime did not respond in time"),
            Self::HookFailed(reason) => write!(formatter, "Provider operation failed: {reason}"),
            Self::Refused(reason) => write!(formatter, "Provider refused: {reason}"),
            Self::PrewriteRefused(reason) => write!(formatter, "Input was not sent: {reason}"),
            Self::Superseded => {
                formatter.write_str("Runtime session changed; this operation was cancelled")
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuntimePhase {
    Unloaded { reason: Option<String> },
    Loading,
    Ready,
    Degraded { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub epoch: u64,
    pub thread_id: Option<String>,
    pub phase: RuntimePhase,
    pub active_turn_started_at_ms: Option<u64>,
    pub last_turn_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub state: String,
    pub epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_started_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_duration_ms: Option<u64>,
}

impl RuntimeState {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        let (state, reason) = match &self.phase {
            RuntimePhase::Unloaded { reason } => ("unloaded", reason.clone()),
            RuntimePhase::Loading => ("loading", None),
            RuntimePhase::Ready => ("loaded", None),
            RuntimePhase::Degraded { reason } => ("degraded", Some(reason.clone())),
        };
        RuntimeSnapshot {
            state: state.into(),
            epoch: self.epoch,
            reason,
            active_turn_started_at_ms: self.active_turn_started_at_ms,
            last_turn_duration_ms: self.last_turn_duration_ms,
        }
    }

    pub fn unloaded(thread_id: Option<String>) -> Self {
        Self {
            epoch: 0,
            thread_id,
            phase: RuntimePhase::Unloaded { reason: None },
            active_turn_started_at_ms: None,
            last_turn_duration_ms: None,
        }
    }
    pub fn restore_timing(&mut self, snapshot: Option<&serde_json::Value>) {
        let Some(snapshot) = snapshot else {
            return;
        };
        let safe = |field: &str| {
            snapshot[field]
                .as_u64()
                .filter(|value| *value <= 9_007_199_254_740_991)
        };
        self.epoch = safe("epoch").unwrap_or_default();
        self.active_turn_started_at_ms = safe("active_turn_started_at_ms");
        self.last_turn_duration_ms = safe("last_turn_duration_ms");
    }
    pub fn working(&mut self, same_turn: bool, at: u64, started_at: Option<u64>) {
        if !same_turn || self.active_turn_started_at_ms.is_none() {
            self.active_turn_started_at_ms = Some(started_at.unwrap_or(at));
        }
    }
    pub fn completed(&mut self, at: u64, completed_at: Option<u64>) {
        if let Some(start) = self.active_turn_started_at_ms.take() {
            self.last_turn_duration_ms = Some(completed_at.unwrap_or(at).saturating_sub(start));
        }
    }
    pub fn begin(&mut self) -> u64 {
        self.epoch = self.epoch.wrapping_add(1);
        self.phase = RuntimePhase::Loading;
        self.epoch
    }
    pub fn load(&mut self, epoch: u64, thread: &str) -> bool {
        if self.epoch != epoch
            || self
                .thread_id
                .as_deref()
                .is_some_and(|bound| bound != thread)
        {
            return false;
        }
        self.thread_id = Some(thread.into());
        self.phase = RuntimePhase::Ready;
        true
    }
    pub fn unload(&mut self, epoch: u64, reason: &RuntimeFailure) -> bool {
        if self.epoch != epoch || matches!(self.phase, RuntimePhase::Unloaded { .. }) {
            return false;
        }
        self.phase = RuntimePhase::Unloaded {
            reason: Some(reason.to_string()),
        };
        true
    }
    pub fn degrade(&mut self, epoch: u64, reason: &RuntimeFailure) -> bool {
        if self.epoch != epoch {
            return false;
        }
        self.phase = RuntimePhase::Degraded {
            reason: reason.to_string(),
        };
        true
    }
}

#[derive(Default)]
pub struct ItemCache {
    items: Vec<ChatMsg>,
    positions: HashMap<String, usize>,
}
impl ItemCache {
    pub fn reconcile(&mut self, mut item: ChatMsg) -> ChatMsg {
        if let Some(index) = self.positions.get(&item.id).copied() {
            item.seq = self.items[index].seq;
            self.items[index] = item.clone();
        } else {
            item.seq = self.items.len() as i64;
            self.positions.insert(item.id.clone(), self.items.len());
            self.items.push(item.clone());
        }
        item
    }
    pub fn snapshot(&self) -> Vec<ChatMsg> {
        self.items.clone()
    }
}

pub fn validate_operation_thread(
    context: &codeterm_plugin::agent_runtime::RuntimeContext,
    operation: &codeterm_plugin::agent_runtime::RuntimeOperation,
) -> Result<(), RuntimeFailure> {
    use codeterm_plugin::agent_runtime::RuntimeOperation;
    let target = match operation {
        RuntimeOperation::ResumeThread { thread_id, .. }
        | RuntimeOperation::AnswerQuestion { thread_id, .. }
        | RuntimeOperation::ReadHistory { thread_id, .. }
        | RuntimeOperation::SubmitTurn { thread_id, .. }
        | RuntimeOperation::Interrupt { thread_id, .. } => Some(thread_id.as_str()),
        RuntimeOperation::Control { thread_id, .. } => thread_id.as_deref(),
        RuntimeOperation::Initialize
        | RuntimeOperation::OpenThread { .. }
        | RuntimeOperation::Answer { .. } => None,
    };
    if target.is_some() && target != context.thread_id.as_deref() {
        return Err(RuntimeFailure::Superseded);
    }
    let active = match operation {
        RuntimeOperation::SubmitTurn { active_turn_id, .. }
        | RuntimeOperation::Interrupt { active_turn_id, .. } => active_turn_id.as_deref(),
        _ => None,
    };
    if active.is_some() && active != context.active_turn_id.as_deref() {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(())
}

pub fn permits(
    capability: &codeterm_plugin::agent_runtime::AgentRuntimeCapability,
    operation: &codeterm_plugin::agent_runtime::RuntimeOperation,
    method: &str,
) -> bool {
    capability
        .request_methods
        .get(operation.kind())
        .is_some_and(|methods| methods.iter().any(|allowed| allowed == method))
}

pub fn settings_from_config(
    config: &serde_json::Value,
) -> codeterm_plugin::agent_runtime::RuntimeSettings {
    codeterm_plugin::agent_runtime::RuntimeSettings {
        model: config["model"].as_str().map(str::to_string),
        effort: config["effort"].as_str().map(str::to_string),
        mode: config["mode"].as_str().map(str::to_string),
        additional_directories: config["additionalDirectories"]
            .as_array()
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| path.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default(),
        network_access: config["networkAccess"].as_bool(),
    }
}

pub fn public_provider_settings(config: &serde_json::Value) -> serde_json::Value {
    match config {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
                    if ["token", "password", "secret", "credential", "apikey"]
                        .iter()
                        .any(|part| normalized.contains(part))
                    {
                        None
                    } else {
                        Some((key.clone(), public_provider_settings(value)))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(public_provider_settings).collect())
        }
        value => value.clone(),
    }
}

pub fn validate_state_size(state: &serde_json::Value) -> Result<(), RuntimeFailure> {
    if serde_json::to_vec(state)
        .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))?
        .len()
        > codeterm_plugin::agent_runtime::MAX_RUNTIME_STATE_BYTES
    {
        return Err(RuntimeFailure::Refused(
            "Runtime hook state size limit".into(),
        ));
    }
    Ok(())
}

pub fn validate_prompt_answer(
    prompt: &codeterm_types::pty::PendingPrompt,
    answer: &codeterm_plugin::agent_runtime::PromptAnswer,
) -> Result<(), RuntimeFailure> {
    use codeterm_plugin::agent_runtime::PromptAnswer;
    if let PromptAnswer::Choice { values } = answer {
        let unique = values.iter().collect::<std::collections::HashSet<_>>();
        if values.is_empty()
            || unique.len() != values.len()
            || (!prompt.multi_select.unwrap_or(false) && values.len() > 1)
            || values
                .iter()
                .any(|value| !prompt.options.iter().any(|option| &option.value == value))
        {
            return Err(RuntimeFailure::Refused(
                "Answer is outside the own prompt options".into(),
            ));
        }
    }
    Ok(())
}

pub fn validated_evidence(
    claims: &[codeterm_plugin::agent_runtime::ClaimedSend],
    events: &[codeterm_plugin::agent_runtime::AgentRuntimeEvent],
) -> std::collections::HashSet<String> {
    use codeterm_plugin::agent_runtime::AgentRuntimeEvent;
    events.iter().filter_map(|event| {
        let AgentRuntimeEvent::SendEvidence { thread_id, send_id, turn_id } = event else { return None };
        if turn_id.is_empty() { return None; }
        let claim = claims.iter().find(|claim| claim.send_id == *send_id && claim.thread_id == *thread_id)?;
        let exact = events.iter().any(|item| matches!(item,
            AgentRuntimeEvent::ChatItem { thread_id: own, item }
                if own == thread_id && item.kind == "user" && item.content == claim.input.text
                    && item.attachments.as_deref().unwrap_or_default() == claim.input.attachments.as_slice()));
        exact.then(|| send_id.clone())
    }).collect()
}

pub fn accepts_turn_transition(
    current: Option<&str>,
    incoming: Option<&str>,
    state: &codeterm_plugin::agent_runtime::RuntimeTurnState,
) -> bool {
    !matches!(
        state,
        codeterm_plugin::agent_runtime::RuntimeTurnState::Idle
    ) || current.is_none()
        || incoming.is_none()
        || current == incoming
}

pub fn observe_turn(
    context: &mut codeterm_plugin::agent_runtime::RuntimeContext,
    event: &codeterm_plugin::agent_runtime::AgentRuntimeEvent,
) {
    use codeterm_plugin::agent_runtime::{
        AgentRuntimeEvent, RuntimeActiveTurn, RuntimeThreadParent, RuntimeTurnState,
    };
    let AgentRuntimeEvent::TurnState {
        thread_id,
        turn_id,
        parent_thread_id,
        state,
        ..
    } = event
    else {
        return;
    };
    if context.thread_id.as_ref() != Some(thread_id)
        && !context
            .thread_parents
            .iter()
            .any(|parent| &parent.thread_id == thread_id)
    {
        if let Some(parent) = parent_thread_id {
            context.thread_parents.push(RuntimeThreadParent {
                thread_id: thread_id.clone(),
                parent_thread_id: parent.clone(),
            });
        }
    }
    if matches!(state, RuntimeTurnState::Idle) {
        context.active_turns.retain(|turn| {
            turn.thread_id != *thread_id || turn_id.as_ref().is_some_and(|id| id != &turn.turn_id)
        });
    } else if let Some(turn_id) = turn_id {
        context
            .active_turns
            .retain(|turn| turn.thread_id != *thread_id);
        let parent = context
            .thread_parents
            .iter()
            .find(|parent| &parent.thread_id == thread_id)
            .map(|parent| parent.parent_thread_id.clone());
        context.active_turns.push(RuntimeActiveTurn {
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            parent_thread_id: parent,
        });
    }
}

#[cfg(test)]
mod tests;
