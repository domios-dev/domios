use codeterm_plugin::agent_runtime::RpcId;
use codeterm_plugin::agent_runtime::{
    AgentRuntimeEvent, RuntimeAuthState, RuntimeContext, RuntimeLoginState, RuntimeQuestion,
    RuntimeQuestionResponse, RuntimeRateLimit, RuntimeSettings, RuntimeTurnDetail,
    RuntimeTurnState, TokenUsage,
};
use codeterm_types::agent::chat_types::ChatMsg;
use codeterm_types::pty::PendingPrompt;
use std::collections::HashMap;

pub const LIVE_ITEM_CAP: usize = 1000;
pub const LIVE_ITEM_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    SessionUpdated {
        login: Option<RuntimeLoginState>,
        auth: Option<RuntimeAuthState>,
        billing_mode: Option<codeterm_types::usage::BillingMode>,
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
        resolved: bool,
    },
    EvidenceUpdated {
        thread_id: String,
        send_id: String,
        turn_id: String,
    },
    UsageUpdated {
        thread_id: String,
        usage: TokenUsage,
    },
    RateLimitsUpdated {
        thread_id: Option<String>,
        rate_limits: Vec<RuntimeRateLimit>,
    },
    CompactionUpdated {
        thread_id: String,
        item_id: String,
    },
    Warning {
        id: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
struct StoredItem {
    thread_id: String,
    item: ChatMsg,
    closed: bool,
}

#[derive(Clone, Debug, Default)]
struct TurnSnap {
    state: Option<RuntimeTurnState>,
    parent_thread_id: Option<String>,
}

#[derive(Default)]
pub struct RuntimeMaps {
    epoch: u64,
    items: Vec<StoredItem>,
    turns: HashMap<(String, String), TurnSnap>,
    threads: HashMap<String, Option<String>>,
    requests: HashMap<String, bool>,
    evidence: HashMap<String, (String, String)>,
    usage: HashMap<String, TokenUsage>,
    rate_limits: HashMap<String, Vec<RuntimeRateLimit>>,
    auth: Option<RuntimeAuthState>,
}

impl RuntimeMaps {
    pub fn reset(&mut self, epoch: u64) {
        *self = Self {
            epoch,
            ..Self::default()
        };
    }

    pub fn item_ids(&self) -> Vec<String> {
        self.items.iter().map(|item| item.item.id.clone()).collect()
    }

    pub fn text_bytes(&self) -> usize {
        self.items.iter().map(|item| text_bytes(&item.item)).sum()
    }

    pub fn take_evidence(&mut self, send_id: &str) -> Option<(String, String)> {
        self.evidence.remove(send_id)
    }
}

pub fn translate(event: &AgentRuntimeEvent) -> RuntimeEvent {
    match event {
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
            resolved: false,
        },
        AgentRuntimeEvent::RateLimits {
            thread_id,
            rate_limits,
        } => RuntimeEvent::RateLimitsUpdated {
            thread_id: thread_id.clone(),
            rate_limits: rate_limits.clone(),
        },
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
            thread_id,
            send_id,
            turn_id,
        } => RuntimeEvent::EvidenceUpdated {
            thread_id: thread_id.clone(),
            send_id: send_id.clone(),
            turn_id: turn_id.clone(),
        },
        AgentRuntimeEvent::Usage { thread_id, usage } => RuntimeEvent::UsageUpdated {
            thread_id: thread_id.clone(),
            usage: usage.clone(),
        },
        AgentRuntimeEvent::Prompt {
            thread_id,
            request_id,
            prompt,
        } => RuntimeEvent::RequestUpdated {
            thread_id: thread_id.clone(),
            request_id: Some(request_id.clone()),
            item_id: None,
            questions: None,
            response: None,
            prompt: Some(prompt.clone()),
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
    }
}

pub fn fold(maps: &mut RuntimeMaps, epoch: u64, event: &RuntimeEvent) {
    if maps.epoch != epoch {
        maps.reset(epoch);
    }
    match event {
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
            turn_id,
            state,
            parent_thread_id,
            ..
        } => {
            let key = (thread_id.clone(), turn_id.clone().unwrap_or_default());
            maps.turns.insert(
                key,
                TurnSnap {
                    state: Some(state.clone()),
                    parent_thread_id: parent_thread_id.clone(),
                },
            );
        }
        RuntimeEvent::ItemUpdated { thread_id, item } => {
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
            thread_id,
            send_id,
            turn_id,
        } => {
            maps.evidence
                .entry(send_id.clone())
                .or_insert_with(|| (thread_id.clone(), turn_id.clone()));
        }
        RuntimeEvent::UsageUpdated { thread_id, usage } => {
            maps.usage.insert(thread_id.clone(), usage.clone());
        }
        RuntimeEvent::RateLimitsUpdated {
            thread_id,
            rate_limits,
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

pub fn observe(context: &mut RuntimeContext, event: &RuntimeEvent) {
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
    if let Some(stored) = maps
        .items
        .iter_mut()
        .find(|stored| stored.thread_id == thread_id && stored.item.id == item.id)
    {
        stored.item = item;
        stored.closed = closed;
    } else {
        maps.items.push(StoredItem {
            thread_id: thread_id.to_string(),
            item,
            closed,
        });
    }
    bound_items(&mut maps.items, LIVE_ITEM_CAP, LIVE_ITEM_BYTES);
}

fn bound_items(items: &mut Vec<StoredItem>, max_items: usize, max_bytes: usize) {
    while items.len() > max_items || text_total(items) > max_bytes {
        if let Some(index) = items.iter().position(|item| item.closed) {
            items.remove(index);
            continue;
        }
        if text_total(items) > max_bytes && shrink_heaviest(items) {
            continue;
        }
        if items.len() > max_items {
            items.remove(0);
            continue;
        }
        break;
    }
}

fn text_total(items: &[StoredItem]) -> usize {
    items.iter().map(|item| text_bytes(&item.item)).sum()
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
