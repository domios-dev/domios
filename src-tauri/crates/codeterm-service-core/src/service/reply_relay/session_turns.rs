use crate::agent::provider::ReplyRelayEvidence;
use crate::service::ServiceContext;
use serde::Serialize;
use std::collections::VecDeque;

pub const MAX_SCAN_TURNS: usize = 256;
pub const MAX_TURN_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct SessionTurn {
    user_turn_id: String,
    user_record_start: u64,
    user_text: String,
    assistant_turn_id: Option<String>,
    assistant_text: Option<String>,
    outcome: String,
}

#[derive(Debug, Serialize)]
pub struct SessionTurns {
    tab_id: String,
    session_id: String,
    turns: Vec<SessionTurn>,
    next_offset: u64,
    has_more: bool,
    unavailable_reason: Option<String>,
}

pub fn may_read_turns(
    caller: Option<&str>,
    target: &str,
    authenticated: bool,
    manager: Option<&str>,
    plan_owner: Option<&str>,
) -> bool {
    match caller {
        None => true,
        Some(caller) => {
            authenticated
                && (caller == target || manager == Some(caller) || plan_owner == Some(caller))
        }
    }
}

pub fn collect_turns(
    last: u64,
    after: u64,
    mut read: impl FnMut(u64) -> Option<ReplyRelayEvidence>,
) -> Result<SessionTurns, String> {
    if !(1..=50).contains(&last) {
        return Err("last must be between 1 and 50".into());
    }
    let mut rows = VecDeque::new();
    let mut offset = after;
    let mut bytes = 0;
    let mut unavailable_reason = None;
    let mut has_more = false;
    for index in 0..=MAX_SCAN_TURNS {
        let Some(evidence) = read(offset) else { break };
        if index == MAX_SCAN_TURNS {
            has_more = true;
            break;
        }
        let row = match evidence {
            ReplyRelayEvidence::Answered(turn)
                if turn.complete
                    && !turn.user_turn_id.is_empty()
                    && !turn.assistant_turn_id.is_empty() =>
            {
                SessionTurn {
                    user_turn_id: turn.user_turn_id,
                    user_record_start: turn.user_record_start,
                    user_text: turn.user_text,
                    assistant_turn_id: Some(turn.assistant_turn_id),
                    assistant_text: Some(turn.answer),
                    outcome: "answered".into(),
                }
            }
            ReplyRelayEvidence::Ended(turn) if !turn.complete && !turn.user_turn_id.is_empty() => {
                SessionTurn {
                    user_turn_id: turn.user_turn_id,
                    user_record_start: turn.user_record_start,
                    user_text: turn.user_text,
                    assistant_turn_id: None,
                    assistant_text: None,
                    outcome: turn.outcome.as_str().into(),
                }
            }
            ReplyRelayEvidence::Unavailable(state)
                if state.unavailable && !state.reason.trim().is_empty() =>
            {
                unavailable_reason = Some(state.reason);
                break;
            }
            _ => return Err("provider returned invalid completed-turn evidence".into()),
        };
        if row.user_record_start < offset {
            return Err("provider returned a non-progressing session-turn cursor".into());
        }
        offset = row
            .user_record_start
            .checked_add(1)
            .ok_or("session-turn cursor overflow")?;
        let size = serde_json::to_vec(&row)
            .map_err(|error| error.to_string())?
            .len();
        if size > MAX_TURN_BYTES {
            return Err("completed turn exceeds 256 KiB read budget".into());
        }
        bytes += size;
        rows.push_back((row, size));
        if rows.len() > last as usize {
            if let Some((_, size)) = rows.pop_front() {
                bytes -= size;
            }
        }
        if bytes > MAX_RESPONSE_BYTES {
            return Err("selected turns exceed 1 MiB response budget; request fewer turns".into());
        }
    }
    Ok(SessionTurns {
        tab_id: String::new(),
        session_id: String::new(),
        turns: rows.into_iter().map(|(row, _)| row).collect(),
        next_offset: offset,
        has_more,
        unavailable_reason,
    })
}

impl ServiceContext {
    pub fn read_tool_less_session_turns(
        &self,
        target: &str,
        last: u64,
        after: u64,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<SessionTurns, String> {
        let target = self
            .mux
            .read()
            .resolve_tab_id(target)
            .ok_or("target tab not found")?;
        let task = self.reply_relay_task_binding(&target);
        let owner = task
            .as_ref()
            .and_then(|task| self.plan_store.owner_of(&task.plan_id))
            .and_then(|owner| owner.tab_id);
        {
            let mux = self.mux.read();
            let tab = mux.get_tab(&target).ok_or("target tab closed")?;
            let manager = tab
                .managed
                .as_ref()
                .filter(|managed| managed.is_active())
                .map(|managed| managed.manager_tab_id.as_str());
            let authenticated = caller.and_then(|id| mux.get_tab(id)).is_some_and(|tab| {
                tab.launch_session_authenticated()
                    && nonce.is_some()
                    && tab.session_binding_nonce.as_deref() == nonce
            });
            if !may_read_turns(caller, &target, authenticated, manager, owner.as_deref()) {
                return Err(
                    "session turns require the authenticated manager, plan owner, or target agent"
                        .into(),
                );
            }
        }
        let binding = self
            .current_reply_relay_binding(&target)
            .ok_or("target has no authenticated tool-less reply-turn source")?;
        let mut result = collect_turns(last, after, |offset| {
            self.agent_registry
                .with_provider(&binding.provider, |provider| {
                    provider.read_reply_relay_turn(&binding.cwd, &binding.session, offset)
                })
                .flatten()
        })?;
        if self.current_reply_relay_binding(&target).as_ref() != Some(&binding) {
            return Err("session binding changed during turn read; result discarded".into());
        }
        result.tab_id = target;
        result.session_id = binding.session;
        if serde_json::to_vec(&result)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_RESPONSE_BYTES
        {
            return Err("selected turns exceed 1 MiB response budget; request fewer turns".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
