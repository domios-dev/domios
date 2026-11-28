use codeterm_plugin::agent_runtime::RpcId;
use codeterm_plugin::agent_runtime::{
    AgentRuntimeEvent, RuntimeAccountCredits, RuntimeAuthState, RuntimeContext, RuntimeLoginState,
    RuntimePermission, RuntimeQuestion, RuntimeQuestionResponse, RuntimeRateLimit, RuntimeSettings,
    RuntimeTurnDetail, RuntimeTurnState, TokenUsage,
};
use codeterm_types::agent::chat_types::ChatMsg;
use codeterm_types::pty::PendingPrompt;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

mod routed;

pub const LIVE_ITEM_CAP: usize = 1000;
pub const LIVE_ITEM_BYTES: usize = 8 * 1024 * 1024;
const TERMINAL_SNAPSHOTS: usize = 32;

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    AuthenticationDeclared {
        authentication: codeterm_plugin::agent_runtime::RuntimeAuthentication,
    },
    Notice {
        thread_id: String,
        message: String,
    },
    BackgroundProcessesUpdated,
    SessionUpdated {
        login: Option<RuntimeLoginState>,
        auth: Option<RuntimeAuthState>,
        billing_mode: Option<codeterm_usage::contract::BillingMode>,
        detail: Option<String>,
    },
    ThreadUpdated {
        thread_id: String,
        parent_thread_id: Option<String>,
        settings: Option<RuntimeSettings>,
    },
    TurnUpdated {
        thread_id: String,
        turn_id: Option<String>,
        state: RuntimeTurnState,
        detail: Option<RuntimeTurnDetail>,
        status_line: Option<String>,
        started_at: Option<u64>,
        completed_at: Option<u64>,
        parent_thread_id: Option<String>,
    },
    ItemUpdated {
        thread_id: String,
        item: ChatMsg,
    },
    RequestUpdated {
        thread_id: String,
        request_id: Option<RpcId>,
        item_id: Option<String>,
        questions: Option<Vec<RuntimeQuestion>>,
        response: Option<RuntimeQuestionResponse>,
        prompt: Option<PendingPrompt>,
        permission: Option<RuntimePermission>,
        file_diffs: Option<serde_json::Value>,
        elicitation: Option<serde_json::Value>,
        resolved: bool,
    },
    EvidenceUpdated {
        item_id: Option<String>,
        thread_id: String,
        send_id: String,
        turn_id: String,
    },
    ContextUpdated {
        thread_id: String,
        used_tokens: u64,
        window_tokens: u64,
        cost_micros: Option<u64>,
    },
    UsageUpdated {
        thread_id: String,
        usage: TokenUsage,
    },
    RateLimitsUpdated {
        thread_id: Option<String>,
        rate_limits: Vec<RuntimeRateLimit>,
        resets: Option<codeterm_plugin::agent_runtime::ResetCredits>,
        account_credits: Option<RuntimeAccountCredits>,
    },
    CompactionUpdated {
        thread_id: String,
        item_id: String,
    },
    ThreadReverted {
        thread_id: String,
    },
    SessionTitleUpdated {
        thread_id: String,
        title: String,
    },
    Warning {
        id: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
struct StoredItem {
    turn_id: Option<String>,
    thread_id: String,
    item: ChatMsg,
    closed: bool,
    terminal_output: Option<Arc<TerminalOutput>>,
}

struct RestoreBinding {
    thread: Option<String>,
    nonce: String,
    cwd: String,
}

impl RestoreBinding {
    fn from_context(context: &RuntimeContext) -> Self {
        Self {
            thread: context.thread_id.clone(),
            nonce: context.binding_nonce.clone(),
            cwd: context.cwd.clone(),
        }
    }
    fn matches(&self, context: &RuntimeContext) -> bool {
        self.thread == context.thread_id
            && self.nonce == context.binding_nonce
            && self.cwd == context.cwd
    }
}

struct RefusedClaim {
    turn: String,
    busy_turn: Option<Option<String>>,
}

#[derive(Default)]
pub struct RuntimeMaps {
    pub authentication: Option<codeterm_plugin::agent_runtime::RuntimeAuthentication>,
    epoch: u64,
    restore_binding: Option<RestoreBinding>,
    current_turns: HashMap<String, String>,
    reconciliation_misses: HashSet<(String, String)>,
    items: Vec<StoredItem>,
    terminal_outputs: VecDeque<(String, String, Arc<TerminalOutput>)>,
    threads: HashMap<String, Option<String>>,
    requests: HashMap<String, bool>,
    evidence: HashMap<String, (String, String, Option<String>)>,
    refused_claims: HashMap<String, RefusedClaim>,
    routed_inputs: HashMap<String, (String, ChatMsg)>,
    digest_members: HashMap<String, Vec<String>>,
    usage: HashMap<String, TokenUsage>,
    rate_limits: HashMap<String, Vec<RuntimeRateLimit>>,
    auth: Option<RuntimeAuthState>,
    children: HashMap<String, codeterm_plugin::agent_runtime::RuntimeChild>,
}

#[derive(Debug, PartialEq)]
struct TerminalOutput {
    text: String,
    exit_code: Option<i64>,
}

impl TerminalOutput {
    fn apply(&self, row: &mut ChatMsg) {
        if !self.text.is_empty() {
            match row.tool_result.as_deref().filter(|text| !text.is_empty()) {
                None => row.tool_result = Some(self.text.clone()),
                Some(text) if !text.contains(self.text.as_str()) => {
                    row.tool_result = Some(format!("{text}\n{}", self.text));
                }
                _ => {}
            }
        }
        row.tool_exit_code = row.tool_exit_code.or(self.exit_code);
    }
}

impl RuntimeMaps {
    pub(super) fn remember_terminal_output(
        &mut self,
        context: &RuntimeContext,
        terminal: &str,
        text: String,
        exit_code: Option<i64>,
        finished: bool,
    ) -> Option<ChatMsg> {
        let thread = context.thread_id.as_deref()?;
        if self.epoch != context.epoch
            || !self
                .restore_binding
                .as_ref()
                .is_some_and(|binding| binding.matches(context))
        {
            return None;
        }
        let output = Arc::new(TerminalOutput { text, exit_code });
        if let Some(index) = self
            .terminal_outputs
            .iter()
            .position(|(owner, id, _)| owner == thread && id == terminal)
        {
            if self.terminal_outputs[index].2 == output && !finished {
                return None;
            }
            self.terminal_outputs.remove(index);
        }
        self.terminal_outputs
            .push_back((thread.into(), terminal.into(), output.clone()));
        if self.terminal_outputs.len() > TERMINAL_SNAPSHOTS {
            self.terminal_outputs.pop_front();
        }
        let stored = self.items.iter_mut().rev().find(|stored| {
            stored.thread_id == thread
                && stored.item.kind == "tool_call"
                && stored.item.tool_background_task_id.as_deref() == Some(terminal)
        })?;
        stored.terminal_output = Some(output.clone());
        if stored.closed && !finished {
            return None;
        }
        let mut row = stored.item.clone();
        output.apply(&mut row);
        Some(row)
    }

    fn terminal_output(&self, thread: &str, row: &ChatMsg) -> Option<&Arc<TerminalOutput>> {
        let terminal = row
            .tool_background_task_id
            .as_deref()
            .filter(|_| row.kind == "tool_call")?;
        self.terminal_outputs
            .iter()
            .find(|(owner, id, _)| owner == thread && id == terminal)
            .map(|(_, _, output)| output)
    }

    pub(super) fn project_terminal_output(&self, thread: &str, row: &mut ChatMsg) {
        if row.kind != "tool_call" || row.tool_background_task_id.is_none() {
            return;
        }
        if let Some(output) = self.terminal_output(thread, row).or_else(|| {
            self.items
                .iter()
                .find(|stored| {
                    stored.thread_id == thread
                        && stored.item.id == row.id
                        && stored.item.tool_background_task_id == row.tool_background_task_id
                })
                .and_then(|stored| stored.terminal_output.as_ref())
        }) {
            output.apply(row);
        }
    }

    pub fn child(
        &self,
        epoch: u64,
        thread: &str,
    ) -> Option<codeterm_plugin::agent_runtime::RuntimeChild> {
        (self.epoch == epoch)
            .then(|| self.children.get(thread).cloned())
            .flatten()
    }
    pub fn sync_active_turn(&mut self, context: &RuntimeContext) {
        self.ensure_epoch(context.epoch);
        if self
            .restore_binding
            .as_ref()
            .is_some_and(|binding| !binding.matches(context))
        {
            self.terminal_outputs.clear();
            for stored in &mut self.items {
                stored.terminal_output = None;
            }
        }
        self.routed_inputs.retain(|send, (thread, _)| {
            context
                .claimed_sends
                .iter()
                .any(|claim| &claim.send_id == send && &claim.thread_id == thread)
        });
        self.restore_binding = Some(RestoreBinding::from_context(context));
        if let Some(thread) = &context.thread_id {
            match &context.active_turn_id {
                Some(turn) => {
                    self.current_turns.insert(thread.clone(), turn.clone());
                }
                None => {
                    self.current_turns.remove(thread);
                }
            }
        }
    }
    pub fn prepare_restore(&mut self, context: &RuntimeContext) -> bool {
        if self.epoch > context.epoch
            || self.epoch == context.epoch
                && self
                    .restore_binding
                    .as_ref()
                    .is_some_and(|binding| !binding.matches(context))
        {
            return false;
        }
        self.ensure_epoch(context.epoch);
        self.restore_binding = Some(RestoreBinding::from_context(context));
        true
    }
    pub fn ensure_epoch(&mut self, epoch: u64) {
        if self.epoch != epoch {
            self.reset(epoch);
        }
    }
    pub fn reset(&mut self, epoch: u64) {
        *self = Self {
            epoch,
            ..Self::default()
        };
    }

    pub fn remember_refused_claim(&mut self, epoch: u64, send: &str, turn: &str) {
        if self.epoch == epoch {
            self.refused_claims.insert(
                send.into(),
                RefusedClaim {
                    turn: turn.into(),
                    busy_turn: None,
                },
            );
        }
    }

    pub fn settle_refused_claims(&mut self, epoch: u64, turn: Option<&str>) -> Vec<String> {
        if self.epoch != epoch {
            return Vec::new();
        }
        let mut expired = Vec::new();
        for (send, claim) in &mut self.refused_claims {
            if Some(claim.turn.as_str()) == turn {
                continue;
            }
            // A busy agent can run the refused prompt after the turn it was busy with,
            // so its evidence stays eligible until the turn after that one settles.
            match claim.busy_turn.as_ref().map(|busy| busy.as_deref() == turn) {
                None => claim.busy_turn = Some(turn.map(str::to_owned)),
                Some(true) => {}
                Some(false) => expired.push(send.clone()),
            }
        }
        for send in &expired {
            self.refused_claims.remove(send);
            self.routed_inputs.remove(send);
            self.evidence.remove(send);
        }
        expired
    }

    pub fn take_evidence(&mut self, send_id: &str) -> Option<(String, String, Option<String>)> {
        self.refused_claims.remove(send_id);
        self.evidence.remove(send_id)
    }
}

pub fn translate_with_items(
    event: &AgentRuntimeEvent,
    maps: &mut RuntimeMaps,
    loading: bool,
) -> Option<RuntimeEvent> {
    if let AgentRuntimeEvent::ChatItemPatch { thread_id, item } = event {
        if let Some(stored) = maps.items.iter().find(|stored| {
            stored.thread_id == *thread_id
                && stored.item.id == item.id
                && matches!(
                    stored.item.kind.as_str(),
                    "tool" | "tool_call" | "tool_result"
                )
        }) {
            let mut merged = serde_json::to_value(&stored.item).ok()?;
            let mut patch = serde_json::to_value(item).ok()?;
            let fields = patch.as_object_mut()?;
            for key in ["id", "seq", "type", "content"] {
                fields.remove(key);
            }
            merged.as_object_mut()?.extend(fields.clone());
            let mut next: ChatMsg = serde_json::from_value(merged).ok()?;
            next.seq = stored.item.seq;
            return Some(RuntimeEvent::ItemUpdated {
                thread_id: thread_id.clone(),
                item: next,
            });
        }
        log_reconciliation_miss(maps, thread_id, &item.id, "patch");
        return None;
    }
    if let AgentRuntimeEvent::ChatItemAppend {
        thread_id,
        item_id,
        content,
        offset,
    } = event
    {
        if let Some(stored) = maps
            .items
            .iter()
            .find(|stored| stored.thread_id == *thread_id && stored.item.id == *item_id)
        {
            let current = stored
                .turn_id
                .as_ref()
                .is_some_and(|turn| maps.current_turns.get(thread_id) == Some(turn));
            if (loading || current)
                && matches!(stored.item.kind.as_str(), "user" | "assistant" | "thinking")
                && stored.item.content.len() == *offset
            {
                let mut next = stored.item.clone();
                next.content.push_str(content);
                return Some(RuntimeEvent::ItemUpdated {
                    thread_id: thread_id.clone(),
                    item: next,
                });
            }
        }
        log_reconciliation_miss(maps, thread_id, item_id, "append");
        return None;
    }
    Some(translate(event))
}

fn log_reconciliation_miss(maps: &mut RuntimeMaps, thread: &str, item: &str, operation: &str) {
    if maps.reconciliation_misses.len() < LIVE_ITEM_CAP
        && maps
            .reconciliation_misses
            .insert((thread.into(), item.into()))
    {
        log::debug!("[runtime-stream] reconciliation miss operation={operation} thread={thread} item={item}");
    }
}

pub fn translate(event: &AgentRuntimeEvent) -> RuntimeEvent {
    match event {
        AgentRuntimeEvent::Authentication { authentication } => {
            RuntimeEvent::AuthenticationDeclared {
                authentication: authentication.clone(),
            }
        }
        AgentRuntimeEvent::Notice { thread_id, message } => RuntimeEvent::Notice {
            thread_id: thread_id.clone(),
            message: message.clone(),
        },
        AgentRuntimeEvent::BackgroundProcesses { .. } => RuntimeEvent::BackgroundProcessesUpdated,
        AgentRuntimeEvent::AuthLogin { login } => RuntimeEvent::SessionUpdated {
            login: Some(login.clone()),
            auth: None,
            billing_mode: None,
            detail: None,
        },
        AgentRuntimeEvent::Auth {
            state,
            billing_mode,
            detail,
        } => RuntimeEvent::SessionUpdated {
            login: None,
            auth: Some(state.clone()),
            billing_mode: billing_mode.clone(),
            detail: detail.clone(),
        },
        AgentRuntimeEvent::Question {
            thread_id,
            item_id,
            questions,
            response,
        } => RuntimeEvent::RequestUpdated {
            thread_id: thread_id.clone(),
            request_id: None,
            item_id: Some(item_id.clone()),
            questions: Some(questions.clone()),
            response: Some(response.clone()),
            prompt: None,
            permission: None,
            file_diffs: None,
            elicitation: None,
            resolved: false,
        },
        AgentRuntimeEvent::RateLimits {
            thread_id,
            rate_limits,
            resets,
            account_credits,
        } => RuntimeEvent::RateLimitsUpdated {
            thread_id: thread_id.clone(),
            rate_limits: rate_limits.clone(),
            resets: resets.clone(),
            account_credits: account_credits.clone(),
        },
        AgentRuntimeEvent::ChatItemAppend { .. } | AgentRuntimeEvent::ChatItemPatch { .. } => {
            unreachable!("stream updates require translate_with_items")
        }
        AgentRuntimeEvent::ChatItem { thread_id, item } => RuntimeEvent::ItemUpdated {
            thread_id: thread_id.clone(),
            item: item.clone(),
        },
        AgentRuntimeEvent::TurnState {
            thread_id,
            turn_id,
            started_at,
            completed_at,
            parent_thread_id,
            state,
            detail,
            status_line,
        } => RuntimeEvent::TurnUpdated {
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            state: state.clone(),
            detail: *detail,
            status_line: status_line.clone(),
            started_at: *started_at,
            completed_at: *completed_at,
            parent_thread_id: parent_thread_id.clone(),
        },
        AgentRuntimeEvent::SendEvidence {
            item_id,
            thread_id,
            send_id,
            turn_id,
        } => RuntimeEvent::EvidenceUpdated {
            item_id: item_id.clone(),
            thread_id: thread_id.clone(),
            send_id: send_id.clone(),
            turn_id: turn_id.clone(),
        },
        AgentRuntimeEvent::Context {
            thread_id,
            used_tokens,
            window_tokens,
            cost_micros,
        } => RuntimeEvent::ContextUpdated {
            thread_id: thread_id.clone(),
            used_tokens: *used_tokens,
            window_tokens: *window_tokens,
            cost_micros: *cost_micros,
        },
        AgentRuntimeEvent::Usage { thread_id, usage } => RuntimeEvent::UsageUpdated {
            thread_id: thread_id.clone(),
            usage: usage.clone(),
        },
        AgentRuntimeEvent::Prompt {
            thread_id,
            request_id,
            prompt,
            permission,
            file_diffs,
            elicitation,
        } => RuntimeEvent::RequestUpdated {
            thread_id: thread_id.clone(),
            request_id: Some(request_id.clone()),
            item_id: None,
            questions: None,
            response: None,
            prompt: Some(prompt.clone()),
            permission: permission.clone(),
            file_diffs: file_diffs.clone(),
            elicitation: elicitation.clone(),
            resolved: false,
        },
        AgentRuntimeEvent::PromptResolved {
            thread_id,
            request_id,
        } => RuntimeEvent::RequestUpdated {
            thread_id: thread_id.clone(),
            request_id: Some(request_id.clone()),
            item_id: None,
            questions: None,
            response: None,
            prompt: None,
            permission: None,
            file_diffs: None,
            elicitation: None,
            resolved: true,
        },
        AgentRuntimeEvent::ControlsConfirmed {
            thread_id,
            settings,
        } => RuntimeEvent::ThreadUpdated {
            thread_id: thread_id.clone(),
            parent_thread_id: None,
            settings: Some(settings.clone()),
        },
        AgentRuntimeEvent::ContextCompacted { thread_id, item_id } => {
            RuntimeEvent::CompactionUpdated {
                thread_id: thread_id.clone(),
                item_id: item_id.clone(),
            }
        }
        AgentRuntimeEvent::ThreadReverted { thread_id } => RuntimeEvent::ThreadReverted {
            thread_id: thread_id.clone(),
        },
        AgentRuntimeEvent::SessionTitle { thread_id, title } => RuntimeEvent::SessionTitleUpdated {
            thread_id: thread_id.clone(),
            title: title.clone(),
        },
    }
}

pub fn fold(maps: &mut RuntimeMaps, epoch: u64, event: &RuntimeEvent) {
    if maps.epoch != epoch {
        maps.reset(epoch);
    }
    match event {
        RuntimeEvent::AuthenticationDeclared { authentication } => {
            maps.authentication = Some(authentication.clone())
        }
        RuntimeEvent::Notice { .. } | RuntimeEvent::BackgroundProcessesUpdated => {}
        RuntimeEvent::SessionUpdated { auth, .. } => {
            if let Some(auth) = auth {
                maps.auth = Some(auth.clone());
            }
        }
        RuntimeEvent::ThreadUpdated {
            thread_id,
            parent_thread_id,
            ..
        } => {
            maps.threads
                .insert(thread_id.clone(), parent_thread_id.clone());
        }
        RuntimeEvent::TurnUpdated {
            thread_id,
            state: RuntimeTurnState::Idle,
            turn_id,
            ..
        } => {
            if turn_id
                .as_ref()
                .is_none_or(|turn| maps.current_turns.get(thread_id) == Some(turn))
            {
                maps.current_turns.remove(thread_id);
            }
        }
        RuntimeEvent::TurnUpdated {
            thread_id,
            turn_id: Some(turn),
            ..
        } => {
            maps.current_turns.insert(thread_id.clone(), turn.clone());
        }
        RuntimeEvent::TurnUpdated { .. } => {}
        RuntimeEvent::ItemUpdated { thread_id, item } => {
            if let Some(child) = codeterm_plugin::agent_runtime::runtime_child(thread_id, item) {
                if maps.children.len() < 64 || maps.children.contains_key(&child.thread_id) {
                    maps.children.insert(child.thread_id.clone(), child);
                }
            }
            upsert_item(maps, thread_id, item.clone());
        }
        RuntimeEvent::RequestUpdated {
            request_id,
            item_id,
            resolved,
            ..
        } => {
            let key = request_id
                .as_ref()
                .map(request_key)
                .or_else(|| item_id.clone())
                .unwrap_or_default();
            if !key.is_empty() {
                maps.requests.insert(key, *resolved);
            }
        }
        RuntimeEvent::EvidenceUpdated {
            item_id,
            thread_id,
            send_id,
            turn_id,
        } => {
            maps.evidence
                .entry(send_id.clone())
                .or_insert_with(|| (thread_id.clone(), turn_id.clone(), item_id.clone()));
        }
        RuntimeEvent::ContextUpdated { .. } => {}
        RuntimeEvent::UsageUpdated { thread_id, usage } => {
            maps.usage.insert(thread_id.clone(), usage.clone());
        }
        RuntimeEvent::RateLimitsUpdated {
            thread_id,
            rate_limits,
            ..
        } => {
            maps.rate_limits
                .insert(thread_id.clone().unwrap_or_default(), rate_limits.clone());
        }
        RuntimeEvent::CompactionUpdated {
            thread_id, item_id, ..
        } => {
            let mut item = ChatMsg::default();
            item.id = item_id.clone();
            item.kind = "compaction".into();
            upsert_item(maps, thread_id, item);
        }
        RuntimeEvent::ThreadReverted { .. } | RuntimeEvent::SessionTitleUpdated { .. } => {}
        RuntimeEvent::Warning { id, message } => {
            let mut item = ChatMsg::default();
            item.id = id.clone();
            item.kind = "system".into();
            item.content = message.clone();
            item.system_subtype = Some("runtime_warning".into());
            upsert_item(maps, "", item);
        }
    }
}

pub fn reconcile_history(maps: &mut RuntimeMaps, epoch: u64, thread_id: &str, item: ChatMsg) {
    if maps.epoch != epoch {
        maps.reset(epoch);
    }
    upsert_item(maps, thread_id, item);
}

pub fn reconcile_history_current(
    maps: &mut RuntimeMaps,
    context: &RuntimeContext,
    thread_id: &str,
    item: ChatMsg,
) -> bool {
    if maps.epoch != context.epoch
        || !maps
            .restore_binding
            .as_ref()
            .is_some_and(|binding| binding.matches(context))
    {
        return false;
    }
    upsert_item(maps, thread_id, item);
    true
}

pub fn observe(context: &mut RuntimeContext, event: &RuntimeEvent) {
    if let RuntimeEvent::ItemUpdated { thread_id, item } = event {
        if context.thread_id.as_ref() == Some(thread_id) {
            if let Some(child) = codeterm_plugin::agent_runtime::runtime_child(thread_id, item) {
                if context.thread_parents.len() < 64
                    && (context.thread_id.as_ref() == Some(&child.parent_thread_id)
                        || context
                            .thread_parents
                            .iter()
                            .any(|parent| parent.thread_id == child.parent_thread_id))
                    && !context
                        .thread_parents
                        .iter()
                        .any(|parent| parent.thread_id == child.thread_id)
                {
                    context.thread_parents.push(
                        codeterm_plugin::agent_runtime::RuntimeThreadParent {
                            thread_id: child.thread_id,
                            parent_thread_id: child.parent_thread_id,
                        },
                    );
                }
            }
        }
    }
    use codeterm_plugin::agent_runtime::{RuntimeActiveTurn, RuntimeThreadParent};
    let RuntimeEvent::TurnUpdated {
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

fn upsert_item(maps: &mut RuntimeMaps, thread_id: &str, item: ChatMsg) {
    let closed = item.tool_status.as_deref() != Some("running");
    let terminal_output = maps.terminal_output(thread_id, &item).cloned();
    if let Some(stored) = maps
        .items
        .iter_mut()
        .find(|stored| stored.thread_id == thread_id && stored.item.id == item.id)
    {
        if terminal_output.is_some()
            || stored.item.tool_background_task_id != item.tool_background_task_id
        {
            stored.terminal_output = terminal_output;
        }
        stored.item = item;
        stored.closed = closed;
    } else {
        maps.items.push(StoredItem {
            turn_id: maps.current_turns.get(thread_id).cloned(),
            thread_id: thread_id.to_string(),
            item,
            closed,
            terminal_output,
        });
    }
    bound_items(&mut maps.items, LIVE_ITEM_CAP, LIVE_ITEM_BYTES);
}

fn bound_items(items: &mut Vec<StoredItem>, max_items: usize, max_bytes: usize) {
    while items.len() > max_items || retained_bytes(items) > max_bytes {
        if let Some(index) = items.iter().position(|item| item.closed) {
            items.remove(index);
            continue;
        }
        if retained_bytes(items) > max_bytes {
            if let Some(stored) = items
                .iter_mut()
                .find(|stored| stored.item.attachments.is_some())
            {
                stored.item.attachments = None;
                continue;
            }
            if shrink_heaviest(items) {
                continue;
            }
        }
        if items.len() > max_items {
            items.remove(0);
            continue;
        }
        break;
    }
}

fn retained_bytes(items: &[StoredItem]) -> usize {
    items
        .iter()
        .map(|stored| {
            let item = &stored.item;
            text_bytes(item)
                + item.image_data.as_deref().map_or(0, str::len)
                + stored
                    .terminal_output
                    .as_ref()
                    .map_or(0, |output| output.text.len())
                + item
                    .attachments
                    .iter()
                    .flatten()
                    .map(|attachment| {
                        attachment.data.as_deref().map_or(0, str::len)
                            + attachment.url.as_deref().map_or(0, str::len)
                    })
                    .sum::<usize>()
        })
        .sum()
}

fn text_bytes(item: &ChatMsg) -> usize {
    item.content.len()
        + item.tool_result.as_deref().map(str::len).unwrap_or(0)
        + item.tool_detail.as_deref().map(str::len).unwrap_or(0)
}

fn shrink_heaviest(items: &mut [StoredItem]) -> bool {
    let Some(index) = items
        .iter()
        .enumerate()
        .max_by_key(|(_, item)| text_bytes(&item.item))
        .map(|(index, _)| index)
    else {
        return false;
    };
    let before = text_bytes(&items[index].item);
    if before == 0 {
        return false;
    }
    shrink_field(&mut items[index].item, before / 2);
    text_bytes(&items[index].item) < before
}

fn shrink_field(item: &mut ChatMsg, cap: usize) {
    let fields = [
        item.content.len(),
        item.tool_result.as_deref().map(str::len).unwrap_or(0),
        item.tool_detail.as_deref().map(str::len).unwrap_or(0),
    ];
    let largest = fields.iter().copied().max().unwrap_or(0);
    if largest == 0 {
        return;
    }
    let keep = cap.max(1);
    if item.content.len() == largest {
        item.content = tail(&item.content, keep);
    } else if item.tool_result.as_deref().map(str::len) == Some(largest) {
        if let Some(value) = item.tool_result.as_mut() {
            *value = tail(value, keep);
        }
    } else if let Some(value) = item.tool_detail.as_mut() {
        *value = tail(value, keep);
    }
}

fn request_key(id: &RpcId) -> String {
    match id {
        RpcId::String(value) => value.clone(),
        RpcId::Number(value) => value.to_string(),
    }
}

fn tail(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut start = text.len().saturating_sub(cap);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

#[cfg(test)]
#[path = "events/tests.rs"]
mod tests;
