use super::{engine, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    runtime_descendant, HistoryOrder, RuntimeChild, RuntimeContext, RuntimeOperation, RuntimeResult,
};
use codeterm_types::agent::chat_types::ChatMsg;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
