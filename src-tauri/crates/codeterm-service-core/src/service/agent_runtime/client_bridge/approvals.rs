use super::*;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct Approvals {
    scopes: HashMap<RpcId, Scope>,
    writes: Vec<Grant>,
}

struct Scope {
    generation: u64,
    turn: String,
}

struct Grant {
    id: uuid::Uuid,
    scope: Scope,
    path: PathBuf,
    digest: [u8; 32],
}

impl Approvals {
    pub(super) fn clear(&mut self) {
        self.scopes.clear();
        self.writes.clear();
    }
}

pub(in crate::service::agent_runtime) fn remember(context: &RuntimeContext, request: &RpcId) {
    let (Ok(bridge), Some(turn)) = (bridge(context), context.active_turn_id.as_ref()) else {
        return;
    };
    let mut state = bridge.state.lock();
    if state.closed || state.approvals.scopes.len() >= 64 {
        return;
    }
    let generation = state.generation;
    state.approvals.scopes.insert(
        request.clone(),
        Scope {
            generation,
            turn: turn.clone(),
        },
    );
}

pub(in crate::service::agent_runtime) fn forget(context: &RuntimeContext, request: Option<&RpcId>) {
    if let Ok(bridge) = bridge(context) {
        let mut state = bridge.state.lock();
        if let Some(request) = request {
            state.approvals.scopes.remove(request);
        } else {
            state.approvals.clear();
        }
    }
}

fn approved_write<'a>(
    context: &'a RuntimeContext,
    operation: &RuntimeOperation,
    reply: &Value,
) -> Option<(&'a str, &'a str)> {
    let RuntimeOperation::Answer {
        request_id,
        answer: PromptAnswer::Choice { values },
    } = operation
    else {
        return None;
    };
    let pending = context.pending_request.as_ref()?;
    if pending.method != "session/request_permission"
        || &pending.request_id != request_id
        || values.len() != 1
    {
        return None;
    }
    let params = &pending.params;
    let thread = context.thread_id.as_deref()?;
    if params["sessionId"].as_str() != Some(thread)
        || reply["outcome"]["outcome"] != "selected"
        || reply["outcome"]["optionId"].as_str() != Some(values[0].as_str())
    {
        return None;
    }
    let options = params["options"].as_array()?;
    if options.len() > 16 {
        return None;
    }
    let selected: Vec<_> = options
        .iter()
        .filter(|option| option["optionId"].as_str() == Some(values[0].as_str()))
        .collect();
    if selected.len() != 1
        || !matches!(
            selected[0]["kind"].as_str(),
            Some("allow_once" | "allow_always")
        )
    {
        return None;
    }
    let tool = &params["toolCall"];
    if tool["kind"] != "edit" || tool["toolCallId"].as_str()?.is_empty() {
        return None;
    }
    let entries = tool["content"].as_array()?;
    if entries.len() > 4 {
        return None;
    }
    let mut diffs = entries.iter().filter(|content| content["type"] == "diff");
    let diff = diffs.next()?;
    if diffs.next().is_some() {
        return None;
    }
    let path = diff["path"].as_str()?;
    let content = diff["newText"].as_str()?;
    if let Some(input) = tool.get("rawInput") {
        for hint in ["path", "file_path", "filePath"] {
            if input
                .get(hint)
                .is_some_and(|value| value.as_str() != Some(path))
            {
                return None;
            }
        }
    }
    if content.len() > fs::MAX_BYTES {
        return None;
    }
    if let Some(locations) = tool.get("locations") {
        if locations.as_array()?.len() > 4
            || locations
                .as_array()?
                .iter()
                .any(|location| location["path"].as_str() != Some(path))
        {
            return None;
        }
    }
    Some((path, content))
}

pub(in crate::service::agent_runtime) async fn approved_reply(
    context: &RuntimeContext,
    operation: &RuntimeOperation,
    reply: Option<&Value>,
) -> Option<uuid::Uuid> {
    if context.pending_request.as_ref()?.method != "session/request_permission" {
        return None;
    }
    let Some((path, content)) = reply.and_then(|reply| approved_write(context, operation, reply))
    else {
        forget(context, None);
        return None;
    };
    let bridge = bridge(context).ok()?;
    let relative = bridge.file_relative(path).ok()?;
    if fs::machine_secret(&relative) || !bridge.capability.fs_write {
        return None;
    }
    let content = content.to_owned();
    let request = context.pending_request.as_ref()?.request_id.clone();
    let turn = context.active_turn_id.clone();
    let thread = context.thread_id.clone();
    tokio::task::spawn_blocking(move || {
        let digest = Sha256::digest(content.as_bytes()).into();
        let mut state = bridge.state.lock();
        let scope = state.approvals.scopes.remove(&request)?;
        if state.closed
            || state.generation != scope.generation
            || turn.as_deref() != Some(scope.turn.as_str())
            || state.session != thread
            || state.approvals.writes.len() >= 64
        {
            return None;
        }
        let id = uuid::Uuid::new_v4();
        state.approvals.writes.push(Grant {
            id,
            scope,
            path: relative,
            digest,
        });
        Some(id)
    })
    .await
    .ok()
    .flatten()
}

pub(in crate::service::agent_runtime) fn revoke(context: &RuntimeContext, id: Option<uuid::Uuid>) {
    if let (Ok(bridge), Some(id)) = (bridge(context), id) {
        bridge
            .state
            .lock()
            .approvals
            .writes
            .retain(|grant| grant.id != id);
    }
}

pub(super) fn consume(
    bridge: &Bridge,
    context: &RuntimeContext,
    path: &str,
    content: &str,
) -> bool {
    if bridge.epoch != context.epoch || bridge.nonce != context.binding_nonce {
        return false;
    }
    let Ok(relative) = bridge.file_relative(path) else {
        return false;
    };
    let digest: [u8; 32] = Sha256::digest(content.as_bytes()).into();
    let mut state = bridge.state.lock();
    if state.closed || state.session != context.thread_id {
        return false;
    }
    let matching = state.approvals.writes.iter().position(|grant| {
        grant.scope.generation == state.generation
            && context.active_turn_id.as_deref() == Some(grant.scope.turn.as_str())
            && grant.path == relative
            && grant.digest == digest
    });
    if let Some(index) = matching {
        state.approvals.writes.remove(index);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests;
