use super::super::engine::{self, Session};
use super::super::events::{self, RuntimeEvent};
use super::super::model::RuntimeFailure;
use super::super::process::Process;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use codeterm_types::agent::chat_types::ChatMsg;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

pub const CONSECUTIVE_CLOSE: u32 = 8;

static WARNING_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn warning_id(epoch: u64) -> String {
    let seq = WARNING_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("runtime-warning-{epoch}-{seq}")
}

pub enum Step {
    Continue,
    Break,
    Close(RuntimeFailure),
}

pub fn reset(session: &Session) {
    session.bookkeeping_failures.store(0, Ordering::Relaxed);
}

pub async fn account(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    reason: &str,
) -> Step {
    let next = session.bookkeeping_failures.fetch_add(1, Ordering::Relaxed) + 1;
    emit_warning(svc, session, epoch, reason);
    if next >= CONSECUTIVE_CLOSE {
        return Step::Close(RuntimeFailure::HookFailed(
            "eight consecutive runtime bookkeeping failures".into(),
        ));
    }
    match resync(svc, session, process).await {
        Ok(()) => Step::Continue,
        Err(reason) if protocol(&reason) => Step::Close(reason),
        Err(RuntimeFailure::Superseded) => Step::Break,
        Err(_) => Step::Continue,
    }
}

fn protocol(reason: &RuntimeFailure) -> bool {
    matches!(
        reason,
        RuntimeFailure::PipeEof
            | RuntimeFailure::PipeFailure { .. }
            | RuntimeFailure::ProcessExited { .. }
            | RuntimeFailure::InvalidFrame(_)
    )
}

fn emit_warning(svc: &ServiceContext, session: &Session, epoch: u64, reason: &str) {
    let id = warning_id(epoch);
    let message = format!("Runtime bookkeeping failed: {reason}");
    let mut row = ChatMsg::default();
    row.id = id.clone();
    row.kind = "system".into();
    row.content = message.clone();
    row.system_subtype = Some("runtime_warning".into());
    events::fold(
        &mut session.maps.lock(),
        epoch,
        &RuntimeEvent::Warning {
            id,
            message: message.clone(),
        },
    );
    let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let row = session.cache.lock().reconcile_live(row, at);
    let context = session.context.lock().clone();
    crate::service::tab_chat::structured_sink::emit_chat_msgs(
        &svc.app_handle,
        &context.tab_id,
        &[row],
        None,
        context.thread_id.as_deref(),
        Some(&context.binding_nonce),
    );
}

async fn resync(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let Some(thread_id) = context.thread_id.clone() else {
        return Ok(());
    };
    let instructions = {
        let mux = svc.mux.read();
        mux.get_tab(&context.tab_id)
            .and_then(|tab| tab.agent_runtime_config.as_ref())
            .and_then(|config| config["systemPrompt"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let loaded = engine::run(
        svc,
        &session.provider,
        session,
        process,
        RuntimeOperation::ResumeThread {
            thread_id: thread_id.clone(),
            instructions,
            settings: context.settings.clone(),
        },
    )
    .await?;
    let RuntimeResult::ThreadLoaded {
        thread_id,
        settings,
    } = loaded
    else {
        return Err(RuntimeFailure::HookFailed(
            "Thread load result is invalid".into(),
        ));
    };
    if context.thread_id.as_deref() != Some(thread_id.as_str()) {
        return Err(RuntimeFailure::Superseded);
    }
    session.context.lock().settings = settings;
    let mut cursor = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        let page = engine::run(
            svc,
            &session.provider,
            session,
            process,
            RuntimeOperation::ReadHistory {
                thread_id: thread_id.clone(),
                cursor,
            },
        )
        .await?;
        let RuntimeResult::HistoryPage {
            thread_id: read_thread,
            items,
            next_cursor,
            open_items,
            ..
        } = page
        else {
            return Err(RuntimeFailure::HookFailed(
                "History result is invalid".into(),
            ));
        };
        if read_thread != thread_id {
            return Err(RuntimeFailure::Superseded);
        }
        let epoch = session.context.lock().epoch;
        for item in items {
            session.cache.lock().reconcile(item.clone());
            events::reconcile_history(&mut session.maps.lock(), epoch, &thread_id, item);
        }
        for open in open_items {
            session.cache.lock().reconcile(open.item.clone());
            events::reconcile_history(&mut session.maps.lock(), epoch, &thread_id, open.item);
            session.context.lock().active_turn_id = Some(open.turn_id);
        }
        let Some(next) = next_cursor else {
            break;
        };
        if !seen.insert(next.clone()) {
            return Err(RuntimeFailure::HookFailed("History cursor limit".into()));
        }
        cursor = Some(next);
    }
    Ok(())
}
