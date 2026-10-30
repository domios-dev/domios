use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    ApprovalRequired,
    #[default]
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

pub struct ModelSwitchOptions {
    pub service_tier: Option<codeterm_plugin::agent_runtime::RuntimeServiceTierSelection>,
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
    SignInRequired,
    ConversationMissing,
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
            Self::SignInRequired => formatter.write_str("Sign-in required"),
            Self::ConversationMissing => formatter.write_str("ConversationMissing: The provider's saved conversation rollout is unavailable. Start a new conversation to continue."),
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
    LoginPending,
    Ready,
    Degraded { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub epoch: u64,
    #[serde(
        default,
        rename = "binaryVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub binary_version: Option<String>,
    pub thread_id: Option<String>,
    pub phase: RuntimePhase,
    pub active_turn_started_at_ms: Option<u64>,
    pub last_turn_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub state: String,
    pub epoch: u64,
    #[serde(
        default,
        rename = "binaryVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub binary_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_started_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryBackfill>,
    #[serde(rename = "historyPages", skip_serializing_if = "Option::is_none")]
    pub history_pages: Option<u32>,
    #[serde(rename = "historyError", skip_serializing_if = "Option::is_none")]
    pub history_error: Option<String>,
}

impl RuntimeState {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        let (state, reason) = match &self.phase {
            RuntimePhase::Unloaded { reason } => ("unloaded", reason.clone()),
            RuntimePhase::Loading => ("loading", None),
            RuntimePhase::LoginPending => ("loginPending", None),
            RuntimePhase::Ready => ("loaded", None),
            RuntimePhase::Degraded { reason } => ("degraded", Some(reason.clone())),
        };
        RuntimeSnapshot {
            binary_version: self.binary_version.clone(),
            history: None,
            history_pages: None,
            history_error: None,
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
            binary_version: None,
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
        self.phase = match reason {
            RuntimeFailure::SignInRequired => RuntimePhase::LoginPending,
            _ => RuntimePhase::Degraded {
                reason: reason.to_string(),
            },
        };
        true
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryBackfill {
    Loading,
    #[default]
    Complete,
    Failed,
}

pub const REREADABLE_CACHE_ROWS: usize = 400;
pub const REREADABLE_CACHE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct ItemCache {
    items: VecDeque<ChatMsg>,
    rereadable: bool,
    cached_bytes: usize,
    positions: HashMap<String, i64>,
    head: i64,
    backfill_generation: u64,
    backfill: HistoryBackfill,
    history_pages: u32,
    live_during_backfill: HashSet<String>,
    history_origin: HashSet<String>,
}
impl ItemCache {
    pub fn bound_rereadable(&mut self) {
        self.rereadable = true;
        self.cached_bytes = self.items.iter().map(Self::row_bytes).sum();
        self.evict();
    }
    pub fn retained(&self) -> (usize, usize) {
        (self.items.len(), self.cached_bytes)
    }
    fn row_bytes(row: &ChatMsg) -> usize {
        serde_json::to_vec(row).map_or(usize::MAX, |bytes| bytes.len())
    }
    fn evict(&mut self) {
        while self.rereadable
            && (self.items.len() > REREADABLE_CACHE_ROWS
                || self.cached_bytes > REREADABLE_CACHE_BYTES)
        {
            let Some(row) = self.items.pop_front() else {
                break;
            };
            self.cached_bytes = self.cached_bytes.saturating_sub(Self::row_bytes(&row));
            self.positions.remove(&row.id);
            self.history_origin.remove(&row.id);
            self.live_during_backfill.remove(&row.id);
            self.head += 1;
        }
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.positions
            .get(id)
            .map(|slot| (slot - self.head) as usize)
    }
    /// Starts a replay and drops rows a previous replay landed, keeping live and host rows,
    /// so every history row of this replay can sit below the surviving minimum.
    pub fn begin_backfill(&mut self) -> u64 {
        self.backfill_generation = self.backfill_generation.wrapping_add(1);
        self.backfill = HistoryBackfill::Loading;
        self.history_pages = 0;
        self.live_during_backfill.clear();
        if !self.history_origin.is_empty() {
            let origin = std::mem::take(&mut self.history_origin);
            self.items.retain(|row| !origin.contains(&row.id));
            self.head = 0;
            self.positions = self
                .items
                .iter()
                .enumerate()
                .map(|(index, row)| (row.id.clone(), index as i64))
                .collect();
        }
        if self.rereadable {
            self.cached_bytes = self.items.iter().map(Self::row_bytes).sum();
        }
        self.backfill_generation
    }
    pub fn backfill_current(&self, generation: u64) -> bool {
        self.backfill_generation == generation
    }
    pub fn finish_backfill(&mut self, generation: u64, outcome: HistoryBackfill) -> bool {
        if self.backfill_generation != generation {
            return false;
        }
        self.backfill = outcome;
        if outcome == HistoryBackfill::Complete {
            self.live_during_backfill.clear();
        }
        true
    }
    pub fn resume_backfill(&mut self, generation: u64) -> bool {
        if !self.backfill_current(generation) || self.backfill != HistoryBackfill::Failed {
            return false;
        }
        self.backfill = HistoryBackfill::Loading;
        true
    }
    pub fn backfill(&self) -> HistoryBackfill {
        self.backfill
    }
    pub fn history_pages(&self) -> u32 {
        self.history_pages
    }
    pub fn note_history_page(&mut self) {
        self.history_pages = self.history_pages.saturating_add(1);
    }
    /// Lands history rows older than every cached row without renumbering: known rows update
    /// in place unless a live delta superseded them, unknown rows take seqs below the minimum.
    pub fn reconcile_backfill(&mut self, page: Vec<ChatMsg>) -> usize {
        let mut landed = 0;
        let mut block: Vec<ChatMsg> = Vec::new();
        for mut item in page {
            if self.live_during_backfill.contains(&item.id) {
                continue;
            }
            if self.positions.contains_key(&item.id) {
                self.history_origin.insert(item.id.clone());
                self.reconcile(item);
                landed += 1;
                continue;
            }
            item.refresh_segments();
            match block.iter_mut().find(|row| row.id == item.id) {
                Some(row) => *row = item,
                None => block.push(item),
            }
        }
        landed += block.len();
        let base = self.items.front().map_or(0, |row| row.seq) - block.len() as i64;
        for (offset, mut item) in block.into_iter().enumerate().rev() {
            item.seq = base + offset as i64;
            self.head -= 1;
            self.positions.insert(item.id.clone(), self.head);
            self.history_origin.insert(item.id.clone());
            if self.rereadable {
                self.cached_bytes = self.cached_bytes.saturating_add(Self::row_bytes(&item));
            }
            self.items.push_front(item);
        }
        self.evict();
        landed
    }
    pub fn reconcile_live(&mut self, mut item: ChatMsg, at: u64) -> ChatMsg {
        if self.backfill != HistoryBackfill::Complete {
            self.live_during_backfill.insert(item.id.clone());
        }
        if let Some(status) = item.tool_status.as_deref() {
            let previous = self.index(&item.id).map(|index| &self.items[index]);
            item.tool_started_at_ms = item
                .tool_started_at_ms
                .or_else(|| previous.and_then(|row| row.tool_started_at_ms))
                .or_else(|| (status == "running").then_some(at));
            if status != "running" {
                item.tool_completed_at_ms = item
                    .tool_completed_at_ms
                    .or_else(|| previous.and_then(|row| row.tool_completed_at_ms))
                    .or(Some(at));
            }
        }
        self.reconcile(item)
    }
    pub fn reconcile(&mut self, mut item: ChatMsg) -> ChatMsg {
        item.refresh_segments();
        if let Some(index) = self.index(&item.id) {
            item.seq = self.items[index].seq;
            if self.rereadable {
                self.cached_bytes = self
                    .cached_bytes
                    .saturating_sub(Self::row_bytes(&self.items[index]))
                    .saturating_add(Self::row_bytes(&item));
            }
            self.items[index] = item.clone();
        } else {
            item.seq = self.items.back().map_or(0, |row| row.seq + 1);
            self.positions
                .insert(item.id.clone(), self.head + self.items.len() as i64);
            if self.rereadable {
                self.cached_bytes = self.cached_bytes.saturating_add(Self::row_bytes(&item));
            }
            self.items.push_back(item.clone());
        }
        self.evict();
        item
    }
    pub fn snapshot(&self) -> Vec<ChatMsg> {
        self.items.iter().cloned().collect()
    }
    pub fn get(&self, id: &str) -> Option<&ChatMsg> {
        self.index(id).map(|index| &self.items[index])
    }
    pub fn has_turn(&self, turn: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.tool_flow_id.as_deref() == Some(turn))
    }
    pub fn truncate_after_turn(&mut self, turn: &str) -> bool {
        let Some(end) = self
            .items
            .iter()
            .rposition(|item| item.tool_flow_id.as_deref() == Some(turn))
        else {
            return false;
        };
        while self.items.len() > end + 1 {
            if let Some(item) = self.items.pop_back() {
                self.positions.remove(&item.id);
                self.history_origin.remove(&item.id);
                self.live_during_backfill.remove(&item.id);
                if self.rereadable {
                    self.cached_bytes = self.cached_bytes.saturating_sub(Self::row_bytes(&item));
                }
            }
        }
        true
    }
    pub fn tail(&self, limit: usize) -> Vec<ChatMsg> {
        self.items
            .range(self.items.len().saturating_sub(limit)..)
            .cloned()
            .collect()
    }
}

pub fn validate_operation_thread(
    context: &codeterm_plugin::agent_runtime::RuntimeContext,
    operation: &codeterm_plugin::agent_runtime::RuntimeOperation,
) -> Result<(), RuntimeFailure> {
    use codeterm_plugin::agent_runtime::RuntimeOperation;
    let target = match operation {
        RuntimeOperation::BackgroundProcesses { thread_id, .. } => {
            if !codeterm_plugin::agent_runtime::owns_runtime_thread(context, thread_id) {
                return Err(RuntimeFailure::Superseded);
            }
            None
        }
        RuntimeOperation::ResumeThread { thread_id, .. }
        | RuntimeOperation::AnswerQuestion { thread_id, .. }
        | RuntimeOperation::ReadHistory { thread_id, .. }
        | RuntimeOperation::SubmitTurn { thread_id, .. }
        | RuntimeOperation::Interrupt { thread_id, .. } => Some(thread_id.as_str()),
        RuntimeOperation::Control { thread_id, .. } => thread_id.as_deref(),
        RuntimeOperation::ForkThread { .. } => {
            if context.thread_id.is_some()
                || context.active_turn_id.is_some()
                || !context.active_turns.is_empty()
            {
                return Err(RuntimeFailure::Superseded);
            }
            None
        }
        RuntimeOperation::Initialize
        | RuntimeOperation::OpenThread { .. }
        | RuntimeOperation::Answer { .. } => None,
    };
    if target.is_some()
        && target != context.thread_id.as_deref()
        && !(matches!(operation, RuntimeOperation::ReadHistory { .. })
            && target.is_some_and(|thread| {
                codeterm_plugin::agent_runtime::runtime_descendant(context, thread)
            }))
    {
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
        service_tier: serde_json::from_value(config["serviceTier"].clone()).ok(),
        service_tier_availability: None,
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
    validated_state_bytes(state).map(|_| ())
}

pub(super) fn validated_state_bytes(state: &serde_json::Value) -> Result<usize, RuntimeFailure> {
    let bytes = serde_json::to_vec(state)
        .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))?.len();
    if bytes > codeterm_plugin::agent_runtime::MAX_RUNTIME_STATE_BYTES {
        return Err(RuntimeFailure::Refused("Runtime hook state size limit".into()));
    }
    Ok(bytes)
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
) -> std::collections::HashSet<(String, Option<String>)> {
    use codeterm_plugin::agent_runtime::AgentRuntimeEvent;
    events.iter().filter_map(|event| {
        let AgentRuntimeEvent::SendEvidence { thread_id, send_id, turn_id, item_id } = event else { return None };
        if turn_id.is_empty() || item_id.as_ref().is_some_and(|id| id.is_empty()) { return None; }
        let claim = claims.iter().find(|claim| claim.send_id == *send_id && claim.thread_id == *thread_id)?;
        let exact = item_id.is_none() || events.iter().any(|item| matches!(item,
            AgentRuntimeEvent::ChatItem { thread_id: own, item }
                if own == thread_id && item_id.as_ref().is_none_or(|id| item.id == *id) && item.kind == "user" && item.content == claim.input.text
                    && item.attachments.as_deref().unwrap_or_default() == claim.input.attachments.as_slice()));
        exact.then(|| (send_id.clone(), item_id.clone()))
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

pub fn accepts_response_event(
    before: &codeterm_plugin::agent_runtime::RuntimeContext,
    current: &codeterm_plugin::agent_runtime::RuntimeContext,
    event: &codeterm_plugin::agent_runtime::AgentRuntimeEvent,
) -> bool {
    !matches!(
        event,
        codeterm_plugin::agent_runtime::AgentRuntimeEvent::TurnState { turn_id: None, .. }
    ) || current.active_turn_id == before.active_turn_id
}

#[cfg(test)]
mod tests;

pub(crate) mod responsiveness {
    use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeFrame};
    use parking_lot::Mutex;
    use serde::Deserialize;
    use serde_json::{json, Value};
    use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, OnceLock};
    use std::time::Instant;

    const LIMIT: u64 = 2048;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Selection {
        pub trace: String,
        pub tab: String,
        pub session: String,
        #[serde(default)]
        pub nonce: String,
        #[serde(default)]
        pub epoch: u64,
        #[serde(default)]
        pub turn: String,
        #[serde(default)]
        pub item: String,
        #[serde(default)]
        pub item_type: Option<String>,
    }

    impl Selection {
        pub(crate) fn valid(&self) -> bool {
            let valid_id = |id: &str| id.len() <= 128 && !id.bytes().any(|byte| byte.is_ascii_control());
            [&self.trace, &self.tab, &self.session].iter().all(|id| !id.is_empty() && valid_id(id))
                && [&self.nonce,&self.turn,&self.item].iter().all(|id| valid_id(id))
                && self.item_type.as_deref().is_none_or(valid_id)
                && self.trace.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }
        pub(crate) fn frame_metrics(&self, frame: &RuntimeFrame) -> Option<Value> {
            let (method, params) = match frame {
                RuntimeFrame::Notification { method, params } | RuntimeFrame::Request { method, params, .. } => (method, params),
            };
            if params.get("threadId").and_then(Value::as_str) != Some(self.session.as_str()) { return None; }
            let turn = params.get("turnId").and_then(Value::as_str)
                .or_else(|| params.pointer("/turn/id").and_then(Value::as_str));
            let item = params.get("itemId").and_then(Value::as_str)
                .or_else(|| params.pointer("/item/id").and_then(Value::as_str));
            if (!self.turn.is_empty() && turn != Some(self.turn.as_str())) || (!self.item.is_empty() && item != Some(self.item.as_str())) { return None; }
            Some(json!({"method_hash":fingerprint(&[method]),
                "delta_bytes":params.get("delta").and_then(Value::as_str).map(str::len),
                "content_bytes":params.pointer("/item/text").and_then(Value::as_str).map(str::len),
                "result_bytes":params.pointer("/item/aggregatedOutput").and_then(Value::as_str).map(str::len)}))
        }

        pub(crate) fn matches(&self, context: &RuntimeContext) -> bool {
            self.tab == context.tab_id && Some(self.session.as_str()) == context.thread_id.as_deref()
                && (self.nonce.is_empty() || self.nonce == context.binding_nonce) && (self.epoch == 0 || self.epoch == context.epoch)
        }
    }

    pub(crate) struct Trace {
        selection: Selection,
        started: Instant,
        sequence: AtomicU64,
        dropped: AtomicU64,
        suppressed_observers: AtomicU64,
        overhead_ns: AtomicU64,
        mark_ns: AtomicU64,
        disposed: AtomicBool,
        bound: AtomicBool,
        generation: Mutex<Option<(String, u64)>>,
        target: Mutex<Option<(String, String)>>,
        histogram: Mutex<[u64; 12]>,
    }

    static TRACE: OnceLock<Option<Trace>> = OnceLock::new();

    fn selected() -> Option<&'static Trace> {
        TRACE.get_or_init(|| {
            if !cfg!(debug_assertions) || !crate::paths::is_dev_instance() { return None; }
            let raw = std::env::var("DOMIOS_CHAT_TRACE").ok()?;
            if raw.len() > 2048 { return None; }
            let selection: Selection = serde_json::from_str(&raw).ok()?;
            if !selection.valid() { return None; }
            Some(Trace::new(selection))
        }).as_ref().filter(|trace| !trace.disposed.load(Ordering::Relaxed))
    }

    pub(crate) fn context(context: &RuntimeContext) -> Option<&'static Trace> {
        let trace = selected()?;
        if trace.bind(context) {
            Some(trace)
        } else {
            if trace.bound.load(Ordering::Relaxed) && trace.selection.tab == context.tab_id { trace.dispose(); }
            None
        }
    }

    pub(crate) fn binding(tab: &str, session: Option<&str>, nonce: Option<&str>) -> Option<&'static Trace> {
        selected().filter(|trace| trace.bound.load(Ordering::Relaxed) && trace.selection.tab == tab
            && session == Some(trace.selection.session.as_str())
            && trace.generation.lock().as_ref().is_some_and(|generation| nonce == Some(generation.0.as_str())))
    }

    pub(crate) fn ingress(frame: &RuntimeFrame) {
        if let Some(trace) = selected() { trace.frame(frame); }
    }

    pub(crate) fn dispose() {
        if let Some(trace) = selected() { trace.dispose(); }
    }

    pub(crate) struct Span {
        trace: &'static Trace,
        end: &'static str,
        started: Instant,
        outcome: &'static str,
    }

    impl Span {
        pub(crate) fn finish(&mut self, ok: bool) { self.outcome = if ok { "ok" } else { "error" }; }
    }

    impl Drop for Span {
        fn drop(&mut self) {
            self.trace.mark(self.end, None, json!({"outcome":self.outcome,"duration_us":self.started.elapsed().as_micros()}));
        }
    }

    pub(crate) fn span(context: &RuntimeContext, start: &'static str, end: &'static str) -> Option<Span> {
        let trace = self::context(context)?;
        trace.mark(start, None, json!({}));
        Some(Span { trace, end, started: Instant::now(), outcome: "cancelled_or_unwound" })
    }

    fn fingerprint(parts: &[&str]) -> String {
        let mut hash = 2166136261_u32;
        for part in parts {
            for byte in part.as_bytes().iter().copied().chain(std::iter::once(0)) {
                hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
            }
        }
        format!("{hash:08x}")
    }

    struct Observer<'a> { trace: &'a Trace, started: Instant }

    impl Drop for Observer<'_> {
        fn drop(&mut self) {
            self.trace.overhead_ns.fetch_add(self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64, Ordering::Relaxed);
        }
    }

    impl Trace {
        pub(crate) fn new(selection: Selection) -> Self {
            Self { selection, started: Instant::now(), sequence: AtomicU64::new(0),
                dropped: AtomicU64::new(0), suppressed_observers: AtomicU64::new(0), overhead_ns: AtomicU64::new(0), mark_ns: AtomicU64::new(0),
                disposed: AtomicBool::new(false), bound: AtomicBool::new(false), generation: Mutex::new(None), target: Mutex::new(None), histogram: Mutex::new([0; 12]) }
        }

        pub(crate) fn bind(&self, context: &RuntimeContext) -> bool {
            if context.epoch == 0 || context.binding_nonce.is_empty() || !self.selection.matches(context) { return false; }
            let mut generation = self.generation.lock();
            if let Some((nonce, epoch)) = generation.as_ref() {
                if nonce != &context.binding_nonce || *epoch != context.epoch { return false; }
            } else {
                *generation = Some((context.binding_nonce.clone(), context.epoch));
            }
            self.bound.store(true, Ordering::Relaxed);
            true
        }

        #[cfg(test)]
        pub(crate) fn counts(&self) -> (u64, u64, bool) {
            (self.sequence.load(Ordering::Relaxed), self.dropped.load(Ordering::Relaxed), self.disposed.load(Ordering::Relaxed))
        }
        fn room(&self) -> bool {
            if self.disposed.load(Ordering::Relaxed) { return false; }
            if self.sequence.load(Ordering::Relaxed) >= LIMIT {
                self.suppressed_observers.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            true
        }
        pub(crate) fn mark(&self, stage: &'static str, item: Option<&str>, fields: Value) {
            if self.disposed.load(Ordering::Relaxed) && stage != "dispose" { return; }
            let start = Instant::now();
            let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
            if sequence >= LIMIT && stage != "dispose" {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let selection = &self.selection;
            let generation = self.generation.lock().clone();
            let target = self.target.lock().clone();
            let nonce = generation.as_ref().map(|entry| entry.0.as_str()).unwrap_or(&selection.nonce);
            let epoch = generation.as_ref().map(|entry| entry.1).unwrap_or(selection.epoch);
            log::debug!(target: "chat_responsiveness", "[chat-responsiveness] {}", json!({
                "trace":selection.trace,"stage":stage,"sequence":sequence,
                "clock":"native-instant-us","at_us":self.started.elapsed().as_micros(),
                "unix_ms":chrono::Utc::now().timestamp_millis(),"epoch":epoch,
                "binding":fingerprint(&[&selection.tab,&selection.session,nonce]),
                "turn":target.as_ref().map(|entry| fingerprint(&[&entry.0])),"item":item.map(|id| fingerprint(&[id])),
                "fields":fields,"dropped":self.dropped.load(Ordering::Relaxed),"suppressed_observers":self.suppressed_observers.load(Ordering::Relaxed),
                "observer_ns_before":self.overhead_ns.load(Ordering::Relaxed),"mark_ns_before":self.mark_ns.load(Ordering::Relaxed)
            }));
            self.mark_ns.fetch_add(start.elapsed().as_nanos().min(u64::MAX as u128) as u64, Ordering::Relaxed);
        }

        fn selected_frame(&self, frame: &RuntimeFrame) -> Option<Value> {
            let fields = self.selection.frame_metrics(frame)?;
            let (method, params) = match frame {
                RuntimeFrame::Notification { method, params } | RuntimeFrame::Request { method, params, .. } => (method, params),
            };
            let turn = params.get("turnId").and_then(Value::as_str)?;
            let item = params.get("itemId").and_then(Value::as_str)
                .or_else(|| params.pointer("/item/id").and_then(Value::as_str))?;
            if turn.is_empty() || item.is_empty() || turn.len() > 128 || item.len() > 128 { return None; }
            let mut target = self.target.lock();
            if let Some((selected_turn, selected_item)) = target.as_ref() {
                if selected_turn != turn || selected_item != item { return None; }
            } else {
                if (self.selection.turn.is_empty() || self.selection.item.is_empty())
                    && (method != "item/started" || params.pointer("/item/type").and_then(Value::as_str) == Some("userMessage")) { return None; }
                if self.selection.item_type.as_deref().is_some_and(|kind| params.pointer("/item/type").and_then(Value::as_str) != Some(kind)) { return None; }
                *target = Some((turn.into(), item.into()));
            }
            Some(fields)
        }

        pub(crate) fn frame(&self, frame: &RuntimeFrame) {
            if !self.room() { return; }
            let _observer = Observer { trace: self, started: Instant::now() };
            if let Some(fields) = self.selected_frame(frame) {
                let target = self.target.lock().clone();
                if let Some((turn, item)) = target {
                    self.mark("rpc_ingress", Some(&item), json!({"frame":fields,"turn_id":turn,"item_id":item}));
                }
            }
        }

        pub(crate) fn batch(&self, frames: &[RuntimeFrame], state: &Value) {
            if !self.room() { return; }
            let _observer = Observer { trace: self, started: Instant::now() };
            let count = frames.len();
            let bucket = if count == 0 { 0 } else { (usize::BITS - count.leading_zeros()) as usize }.min(11);
            let histogram = { let mut histogram = self.histogram.lock(); histogram[bucket] += 1; *histogram };
            self.mark("reducer_begin", None, json!({"frames":count,"batch_histogram_log2":histogram,
                "parents":state.get("parents").and_then(Value::as_object).map(|map| map.len()),
                "agents":state.pointer("/agents/rows").and_then(Value::as_object).map(|map| map.len())}));
            for frame in frames {
                if let Some(fields) = self.selected_frame(frame) {
                    let item = self.target.lock().as_ref().map(|entry| entry.1.clone());
                    self.mark("reducer_frame", item.as_deref(), fields);
                }
            }
        }

        pub(crate) fn epoch(&self) -> Option<u64> {
            self.generation.lock().as_ref().map(|generation| generation.1)
        }
        pub(crate) fn row(&self, stage: &'static str, row: &codeterm_types::agent::chat_types::ChatMsg) {
            if !self.room() { return; }
            let _observer = Observer { trace: self, started: Instant::now() };
            let target = self.target.lock().clone();
            let Some((turn,item)) = target else { return; };
            if row.id != item || row.tool_flow_id.as_deref() != Some(turn.as_str()) { return; }
            self.mark(stage, Some(&row.id), json!({"content_bytes":row.content.len(),
                "result_bytes":row.tool_result.as_ref().map(String::len),
                "detail_bytes":row.tool_detail.as_ref().map(String::len),"seq":row.seq,
                "status":row.tool_status.as_deref().filter(|status| ["running","completed","failed","interrupted","abandoned"].contains(status))}));
        }

        pub(crate) fn dispose(&self) {
            if self.disposed.swap(true, Ordering::Relaxed) { return; }
            self.mark("dispose", None, json!({"total_records":self.sequence.load(Ordering::Relaxed),
                "dropped":self.dropped.load(Ordering::Relaxed),"suppressed_observers":self.suppressed_observers.load(Ordering::Relaxed),"observer_ns":self.overhead_ns.load(Ordering::Relaxed),"mark_ns":self.mark_ns.load(Ordering::Relaxed)}));
        }
    }
}
