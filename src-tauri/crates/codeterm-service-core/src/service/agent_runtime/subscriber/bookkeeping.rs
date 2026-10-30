use super::super::engine::{self, Session};
use super::super::events::{self, RuntimeEvent};
use super::super::model::RuntimeFailure;
use super::super::process::Process;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use codeterm_types::agent::chat_types::ChatMsg;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

const MAX_CONSECUTIVE_PROVIDER_OPERATION_FAILURES: u32 = 8;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Lag,
    Hook,
    Publish,
    Limit,
}

impl Reason {
    fn label(self) -> &'static str {
        match self {
            Self::Lag => "lag",
            Self::Hook => "hook",
            Self::Publish => "publish",
            Self::Limit => "limit",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryEvent {
    Bookkeeping(Reason),
    ProviderOperation(Result<(), RuntimeFailure>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    Resync,
    Continue { provider_failures: u32 },
    Break,
    Close(RuntimeFailure),
}

pub(super) fn first_warning(emitted: &AtomicBool) -> bool {
    !emitted.swap(true, Ordering::Relaxed)
}

pub fn resolve(event: RecoveryEvent, provider_failures: u32) -> Resolution {
    match event {
        RecoveryEvent::Bookkeeping(_) => Resolution::Resync,
        RecoveryEvent::ProviderOperation(Ok(())) => Resolution::Continue {
            provider_failures: 0,
        },
        RecoveryEvent::ProviderOperation(Err(reason)) if protocol(&reason) => {
            Resolution::Close(reason)
        }
        RecoveryEvent::ProviderOperation(Err(RuntimeFailure::Superseded)) => Resolution::Break,
        RecoveryEvent::ProviderOperation(Err(reason))
            if matches!(
                reason,
                RuntimeFailure::HookFailed(_) | RuntimeFailure::Deadline
            ) =>
        {
            let provider_failures = provider_failures.saturating_add(1);
            if provider_failures >= MAX_CONSECUTIVE_PROVIDER_OPERATION_FAILURES {
                Resolution::Close(reason)
            } else {
                Resolution::Continue { provider_failures }
            }
        }
        RecoveryEvent::ProviderOperation(Err(_)) => Resolution::Continue { provider_failures },
    }
}

pub async fn account(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    epoch: u64,
    reason: Reason,
) -> Step {
    if first_warning(&session.bookkeeping_warning_emitted) {
        emit_warning(svc, session, epoch, reason.label());
    }
    match resolve(
        RecoveryEvent::Bookkeeping(reason),
        session.provider_operation_failures.load(Ordering::Relaxed),
    ) {
        Resolution::Resync => {
            let context = session.context.lock().clone();
            let mut trace = super::super::model::responsiveness::span(&context, "resync_begin", "resync_end");
            let result = resync(svc, session, process).await;
            if let Some(trace) = &mut trace { trace.finish(result.is_ok()); }
            recovery_step(result, session)
        }
        Resolution::Continue { .. } => Step::Continue,
        Resolution::Break => Step::Break,
        Resolution::Close(reason) => Step::Close(reason),
    }
}

pub(super) fn recovery_step(result: Result<(), RuntimeFailure>, session: &Session) -> Step {
    let failures = session.provider_operation_failures.load(Ordering::Relaxed);
    match resolve(RecoveryEvent::ProviderOperation(result), failures) {
        Resolution::Continue { provider_failures } => {
            session
                .provider_operation_failures
                .store(provider_failures, Ordering::Relaxed);
            Step::Continue
        }
        Resolution::Break => Step::Break,
        Resolution::Close(reason) => Step::Close(reason),
        Resolution::Resync => unreachable!(),
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
        svc.ui.as_ref(),
        &context.tab_id,
        &[row],
        None,
        context.thread_id.as_deref(),
        Some(&context.binding_nonce),
    );
}

pub(super) async fn resync(
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
    let mut resume_trace = super::super::model::responsiveness::span(&context, "resync_resume_begin", "resync_resume_end");
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
    .await;
    if let Some(trace) = &mut resume_trace { trace.finish(loaded.is_ok()); }
    drop(resume_trace);
    let loaded = loaded?;
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
        let mut page_trace = super::super::model::responsiveness::span(&context, "resync_page_begin", "resync_page_end");
        let page = engine::run(
            svc,
            &session.provider,
            session,
            process,
            RuntimeOperation::ReadHistory {
                thread_id: thread_id.clone(),
                cursor,
                order: Some(HistoryOrder::Asc),
            },
        )
        .await;
        if let Some(trace) = &mut page_trace { trace.finish(page.is_ok()); }
        drop(page_trace);
        let page = page?;
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
        for mut item in items {
            super::turn_lifecycle::reconcile_finished_item(session, &thread_id, &mut item);
            session.cache.lock().reconcile(item.clone());
            events::reconcile_history(&mut session.maps.lock(), epoch, &thread_id, item);
        }
        for mut open in open_items {
            super::turn_lifecycle::reconcile_finished_item(session, &thread_id, &mut open.item);
            session.cache.lock().reconcile(open.item.clone());
            events::reconcile_history(&mut session.maps.lock(), epoch, &thread_id, open.item);
            let mut current = session.context.lock();
            if current.active_turn_id.is_none()
                && !session
                    .completions
                    .lock()
                    .contains(&thread_id, &open.turn_id)
            {
                current.active_turn_id = Some(open.turn_id);
            }
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
