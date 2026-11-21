use super::*;
use crate::service::MuxEmitCause;
use serde_json::Value;

const FIELD: &str = "backgroundProcesses";

pub(in crate::service::agent_runtime) fn changed_by(event: &AgentRuntimeEvent) -> bool {
    match event {
        AgentRuntimeEvent::BackgroundProcesses { .. }
        | AgentRuntimeEvent::TurnState {
            state: RuntimeTurnState::Idle,
            ..
        } => true,
        AgentRuntimeEvent::ChatItem { item, .. }
        | AgentRuntimeEvent::ChatItemPatch { item, .. } => {
            item.tool_kind.as_deref() == Some("command") || item.kind == "tool_result"
        }
        _ => false,
    }
}

pub(in crate::service::agent_runtime) fn watch(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
) {
    if !supported(session) {
        super::os_watch::watch(svc, session, process);
        return;
    }
    let (svc, session, process) = (svc.clone(), session.clone(), process.clone());
    let epoch = session.context.lock().epoch;
    let mut ended = process.ended.clone();
    let mut pipe = process.client.ended();
    crate::async_runtime::spawn(async move {
        loop {
            let update = async {
                process.background_changed.notified().await;
                let context = session.context.lock().clone();
                if context.epoch != epoch {
                    return false;
                }
                if let Some(root_thread_id) = context.thread_id.clone() {
                    let authority = BackgroundProcessAuthority {
                        root_thread_id,
                        binding_nonce: context.binding_nonce.clone(),
                        epoch,
                    };
                    if let Err(reason) =
                        refresh(&svc, &session, &process, &context, &authority).await
                    {
                        log::debug!("[background-work] tab={} roster: {reason}", context.tab_id);
                    }
                }
                true
            };
            tokio::select! {
                biased;
                _ = ended.changed() => break,
                _ = pipe.changed() => break,
                keep = update => if !keep { break; },
            }
        }
    });
}

pub(super) async fn refresh(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
    authority: &BackgroundProcessAuthority,
) -> Result<BackgroundProcessSnapshot, RuntimeFailure> {
    let _operation = process.background_operation.lock().await;
    validate_current_session(svc, &context.tab_id, authority, session)?;
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        read_owned(svc, session, process, context),
    )
    .await
    .unwrap_or(Err(RuntimeFailure::Deadline));
    validate_current_session(svc, &context.tab_id, authority, session)?;
    let result = result.map(|processes| snapshot(authority, processes));
    publish(svc, context, result.as_ref().ok());
    result
}

pub(super) fn publish(
    svc: &ServiceContext,
    context: &RuntimeContext,
    roster: Option<&BackgroundProcessSnapshot>,
) {
    let projection = roster
        .filter(|roster| !roster.processes.is_empty())
        .and_then(|roster| serde_json::to_value(roster).ok());
    let changed = {
        let mut mux = svc.mux.write();
        mux.get_tab_mut(&context.tab_id)
            .filter(|tab| {
                tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
                    && tab.agent_session_id == context.thread_id
            })
            .and_then(|tab| tab.agent_runtime_state.as_mut())
            .is_some_and(|state| replace_projection(state, context, projection))
    };
    if changed {
        svc.emit_state_changed(MuxEmitCause::BackgroundWork);
    }
}

fn replace_projection(state: &mut Value, context: &RuntimeContext, roster: Option<Value>) -> bool {
    if !is_loaded(state, context) || state.get(FIELD) == roster.as_ref() {
        return false;
    }
    if let Some(fields) = state.as_object_mut() {
        match roster {
            Some(roster) => {
                fields.insert(FIELD.into(), roster);
            }
            None => {
                fields.remove(FIELD);
            }
        }
    }
    true
}

fn is_loaded(state: &Value, context: &RuntimeContext) -> bool {
    state["state"] == "loaded" && state["epoch"].as_u64() == Some(context.epoch)
}

pub(in crate::service::agent_runtime) fn preserve_projection(
    previous: &Value,
    next: &mut Value,
    context: &RuntimeContext,
) {
    if let Some(roster) = previous.get(FIELD).filter(|roster| {
        roster["rootThreadId"].as_str() == context.thread_id.as_deref()
            && roster["bindingNonce"].as_str() == Some(context.binding_nonce.as_str())
            && roster["epoch"].as_u64() == Some(context.epoch)
    }) {
        replace_projection(next, context, Some(roster.clone()));
    }
}

#[cfg(test)]
mod tests;
