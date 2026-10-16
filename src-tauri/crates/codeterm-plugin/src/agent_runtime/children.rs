use super::{validate_thread_parents, RuntimeAction, RuntimeContext};
use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeChild {
    pub thread_id: String,
    pub parent_thread_id: String,
    pub ordinal: u64,
    pub status: String,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub fn runtime_child(parent: &str, item: &ChatMsg) -> Option<RuntimeChild> {
    if item.tool_kind.as_deref() != Some("agent")
        || item.tool_name.as_deref() != Some("agent_activity")
    {
        return None;
    }
    let input = item.tool_input.as_ref()?.as_object()?;
    let string = |key: &str, max: usize| {
        input
            .get(key)?
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= max)
    };
    let thread = string("agent_id", 4096)?;
    let source = string("agent_parent_thread_id", 4096)?;
    if thread == parent || thread == source {
        return None;
    }
    let ordinal = input
        .get("agent_ordinal")?
        .as_u64()
        .filter(|value| *value > 0 && *value <= 64)?;
    Some(RuntimeChild {
        thread_id: thread.into(),
        parent_thread_id: source.into(),
        ordinal,
        status: string("agent_state", 128).unwrap_or("unknown").into(),
        prompt: string("agent_description", 65536).map(str::to_owned),
        model: string("agent_model", 4096).map(str::to_owned),
        effort: string("agent_reasoning_effort", 128).map(str::to_owned),
    })
}

pub fn runtime_descendant(ctx: &RuntimeContext, thread: &str) -> bool {
    ctx.thread_id.as_deref() != Some(thread)
        && ctx.thread_id.is_some()
        && validate_thread_parents(ctx).is_ok()
        && ctx
            .thread_parents
            .iter()
            .any(|parent| parent.thread_id == thread)
}

pub(super) fn child_history_request(action: &RuntimeAction, thread: &str) -> bool {
    let RuntimeAction::Request { call } = action else {
        return false;
    };
    call.target
        .as_ref()
        .is_some_and(|target| target.thread_id == thread && target.turn_id.is_none())
}

#[cfg(test)]
mod tests;
