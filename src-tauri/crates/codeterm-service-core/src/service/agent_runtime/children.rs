use super::{engine, events, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    runtime_descendant, HistoryOrder, RuntimeChild, RuntimeContext, RuntimeOperation, RuntimeResult,
};
use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc, time::Duration};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChildHistoryRequest {
    pub root_thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildHistoryPage {
    pub child: RuntimeChild,
    pub root_thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub items: Vec<ChatMsg>,
    pub next_cursor: Option<String>,
}

fn authorize(
    ctx: &RuntimeContext,
    request: &ChildHistoryRequest,
    child: &RuntimeChild,
    thread: &str,
) -> Result<(), RuntimeFailure> {
    if ctx.epoch != request.epoch
        || ctx.binding_nonce != request.binding_nonce
        || ctx.thread_id.as_ref() != Some(&request.root_thread_id)
        || child.thread_id != thread
        || !runtime_descendant(ctx, thread)
        || !ctx.thread_parents.iter().any(|parent| {
            parent.thread_id == thread && parent.parent_thread_id == child.parent_thread_id
        })
    {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(())
}

fn restore_relations(
    context: &mut RuntimeContext,
    maps: &mut events::RuntimeMaps,
    root: &str,
    items: &[ChatMsg],
) {
    if context.thread_id.as_deref() != Some(root) {
        return;
    }
    for _ in 0..64 {
        let previous = context.thread_parents.len();
        for item in items {
            let Some(child) = codeterm_plugin::agent_runtime::runtime_child(root, item) else {
                continue;
            };
            if maps.child(context.epoch, &child.thread_id).is_some()
                || (child.parent_thread_id != root
                    && !runtime_descendant(context, &child.parent_thread_id))
            {
                continue;
            }
            let mut item = item.clone();
            if matches!(child.status.as_str(), "started" | "running") {
                item.tool_input.as_mut().unwrap()["agent_state"] = "unknown".into();
            }
            let event = events::RuntimeEvent::ItemUpdated {
                thread_id: root.into(),
                item,
            };
            events::observe(context, &event);
            events::fold(maps, context.epoch, &event);
        }
        if context.thread_parents.len() == previous {
            break;
        }
    }
}

async fn restore_missing(
    svc: &ServiceContext,
    session: &Arc<engine::Session>,
    request: &ChildHistoryRequest,
    thread: &str,
) -> Result<(), RuntimeFailure> {
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::PipeEof)?;
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    loop {
        verify_root(svc, session, request)?;
        let result = engine::run(
            svc,
            &session.provider,
            session,
            &process,
            RuntimeOperation::ReadHistory {
                thread_id: request.root_thread_id.clone(),
                cursor,
                order: Some(HistoryOrder::Desc),
            },
        )
        .await?;
        verify_root(svc, session, request)?;
        let RuntimeResult::HistoryPage {
            thread_id,
            items,
            next_cursor,
            ..
        } = result
        else {
            return Err(RuntimeFailure::Refused(
                "Parent read returned no history".into(),
            ));
        };
        if thread_id != request.root_thread_id {
            return Err(RuntimeFailure::Superseded);
        }
        for item in items {
            let Some(child) = codeterm_plugin::agent_runtime::runtime_child(&thread_id, &item)
            else {
                continue;
            };
            if candidates.len() < 64
                && !candidates.iter().any(|row| {
                    codeterm_plugin::agent_runtime::runtime_child(&thread_id, row)
                        .is_some_and(|known| known.thread_id == child.thread_id)
                })
            {
                candidates.push(item);
            }
        }
        {
            let mut context = session.context.lock();
            if context.epoch != request.epoch
                || context.binding_nonce != request.binding_nonce
                || context.thread_id.as_ref() != Some(&request.root_thread_id)
            {
                return Err(RuntimeFailure::Superseded);
            }
            let mut maps = session.maps.lock();
            restore_relations(&mut context, &mut maps, &thread_id, &candidates);
            if maps.child(request.epoch, thread).is_some() {
                log::debug!("[agent-runtime] restored child relation root={thread_id} child={thread} epoch={}", request.epoch);
                return Ok(());
            }
        }
        let Some(next) = next_cursor else {
            return Err(RuntimeFailure::Superseded);
        };
        if !seen.insert(next.clone()) {
            return Err(RuntimeFailure::Refused(
                "Parent history cursor did not advance".into(),
            ));
        }
        cursor = Some(next);
    }
}

fn verify_root(
    svc: &ServiceContext,
    session: &Arc<engine::Session>,
    request: &ChildHistoryRequest,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    if !engine::for_tab(&context.tab_id).is_some_and(|current| Arc::ptr_eq(&current, session))
        || context.epoch != request.epoch
        || context.binding_nonce != request.binding_nonce
        || context.thread_id.as_ref() != Some(&request.root_thread_id)
    {
        return Err(RuntimeFailure::Superseded);
    }
    engine::validate_binding(svc, session)
}

pub async fn history(
    svc: &ServiceContext,
    tab: &str,
    thread: &str,
    request: ChildHistoryRequest,
) -> Result<ChildHistoryPage, String> {
    history_inner(svc, tab, thread, request)
        .await
        .map_err(|reason| reason.to_string())
}

async fn history_inner(
    svc: &ServiceContext,
    tab: &str,
    thread: &str,
    request: ChildHistoryRequest,
) -> Result<ChildHistoryPage, RuntimeFailure> {
    let session = engine::for_tab(tab).ok_or(RuntimeFailure::Superseded)?;
    verify_root(svc, &session, &request)?;
    let missing = session.maps.lock().child(request.epoch, thread).is_none();
    if missing {
        tokio::time::timeout(
            Duration::from_secs(60),
            restore_missing(svc, &session, &request, thread),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)??;
    }
    let verify = || -> Result<RuntimeChild, RuntimeFailure> {
        if !engine::for_tab(tab).is_some_and(|current| Arc::ptr_eq(&current, &session)) {
            return Err(RuntimeFailure::Superseded);
        }
        engine::validate_binding(svc, &session)?;
        let context = session.context.lock().clone();
        let child = session
            .maps
            .lock()
            .child(context.epoch, thread)
            .ok_or(RuntimeFailure::Superseded)?;
        authorize(&context, &request, &child, thread)?;
        Ok(child)
    };
    verify()?;
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::PipeEof)?;
    let result = engine::run(
        svc,
        &session.provider,
        &session,
        &process,
        RuntimeOperation::ReadHistory {
            thread_id: thread.into(),
            cursor: request.cursor.clone(),
            order: Some(HistoryOrder::Desc),
        },
    )
    .await?;
    let child = verify()?;
    let RuntimeResult::HistoryPage {
        thread_id,
        items,
        next_cursor,
        ..
    } = result
    else {
        return Err(RuntimeFailure::Refused(
            "Child read returned no history".into(),
        ));
    };
    if thread_id != thread {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(ChildHistoryPage {
        child,
        root_thread_id: request.root_thread_id,
        binding_nonce: request.binding_nonce,
        epoch: request.epoch,
        items,
        next_cursor,
    })
}

#[cfg(test)]
mod tests;
