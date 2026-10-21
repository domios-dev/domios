use super::super::engine::{self, Session};
use super::super::events::{self, RuntimeEvent};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeTurnState;
use codeterm_types::agent::chat_types::ChatMsg;

pub(in crate::service::agent_runtime) fn reconcile_finished_item(
    session: &Session,
    thread: &str,
    item: &mut ChatMsg,
) {
    if item.tool_status.as_deref() != Some("running")
        || !item
            .tool_flow_id
            .as_deref()
            .is_some_and(|turn| session.completions.lock().contains(thread, turn))
    {
        return;
    }
    let cache = session.cache.lock();
    let previous = cache.get(&item.id).filter(|row| {
        row.tool_status
            .as_deref()
            .is_some_and(|status| status != "running")
    });
    item.tool_status = previous
        .and_then(|row| row.tool_status.clone())
        .or_else(|| Some("interrupted".into()));
    item.tool_completed_at_ms = previous.and_then(|row| row.tool_completed_at_ms);
    item.tool_interrupted = previous.map_or(Some(true), |row| row.tool_interrupted);
}

pub(super) fn prepare(
    svc: &ServiceContext,
    session: &Session,
    at: u64,
    event: &mut RuntimeEvent,
) -> bool {
    if let RuntimeEvent::ItemUpdated { thread_id, item } = event {
        reconcile_finished_item(session, thread_id, item);
        return true;
    }
    let RuntimeEvent::TurnUpdated {
        thread_id,
        turn_id,
        state,
        ..
    } = event
    else {
        return true;
    };
    let context = session.context.lock().clone();
    let root = context.thread_id.as_ref() == Some(thread_id);
    let current = if root {
        context.active_turn_id.as_deref()
    } else {
        context
            .active_turns
            .iter()
            .find(|turn| &turn.thread_id == thread_id)
            .map(|turn| turn.turn_id.as_str())
    };
    if turn_id.is_none() {
        *turn_id = current.map(str::to_string);
    }
    if !matches!(state, RuntimeTurnState::Idle) {
        if turn_id
            .as_deref()
            .is_some_and(|turn| session.completions.lock().contains(thread_id, turn))
            || (matches!(state, RuntimeTurnState::Working) && turn_id.is_none())
        {
            return false;
        }
        if let Some(previous) = current.filter(|previous| Some(*previous) != turn_id.as_deref()) {
            engine::record_completed_turn(session, thread_id, Some(previous));
            if root {
                settle_tools(svc, session, Some(previous), "abandoned", at);
            }
        }
    } else if root {
        settle_tools(svc, session, turn_id.as_deref(), "interrupted", at);
    }
    true
}

pub(super) fn settle_tools(
    svc: &ServiceContext,
    session: &Session,
    turn: Option<&str>,
    status: &str,
    at: u64,
) {
    let context = session.context.lock().clone();
    let Some(thread) = context.thread_id.as_deref() else {
        return;
    };
    let rows = {
        let mut cache = session.cache.lock();
        cache
            .snapshot()
            .into_iter()
            .filter(|row| {
                row.tool_status.as_deref() == Some("running")
                    && turn.is_none_or(|turn| row.tool_flow_id.as_deref() == Some(turn))
            })
            .map(|mut row| {
                row.tool_status = Some(status.into());
                row.tool_interrupted = Some(true);
                row.tool_completed_at_ms = Some(at);
                cache.reconcile_live(row, at)
            })
            .collect::<Vec<_>>()
    };
    if rows.is_empty() {
        return;
    }
    for item in &rows {
        events::fold(
            &mut session.maps.lock(),
            context.epoch,
            &RuntimeEvent::ItemUpdated {
                thread_id: thread.into(),
                item: item.clone(),
            },
        );
    }
    crate::service::tab_chat::structured_sink::emit_chat_msgs(
        svc.ui.as_ref(),
        &context.tab_id,
        &rows,
        None,
        Some(thread),
        Some(&context.binding_nonce),
    );
}
