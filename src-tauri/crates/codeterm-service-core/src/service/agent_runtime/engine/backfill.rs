use super::{RuntimePhase, Session};
use crate::service::{agent_runtime::model::HistoryBackfill, ServiceContext};
use codeterm_plugin::agent_runtime::*;
use codeterm_types::agent::chat_types::ChatMsg;
use std::{future::Future, sync::Arc, time::Instant};

use super::super::{model::RuntimeFailure, process::Process};

pub(super) const EMITTED_TAIL: usize = 200;

pub(super) trait HistoryPort {
    fn read(
        &mut self,
        cursor: Option<String>,
    ) -> impl Future<Output = Result<RuntimeResult, RuntimeFailure>> + Send;
    fn questions(
        &mut self,
        questions: Vec<RuntimeHistoryQuestion>,
    ) -> impl Future<Output = Result<(), RuntimeFailure>> + Send;
    fn emit(&mut self, rows: &[ChatMsg]);
    fn page_landed(&mut self, _ordinal: u32) {}
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Replayed {
    pub pages: u32,
    pub rows: usize,
}

fn current(session: &Session, epoch: u64, generation: u64, thread: &str) -> bool {
    let state = session.state.lock();
    state.epoch == epoch
        && matches!(state.phase, RuntimePhase::Ready)
        && session.context.lock().thread_id.as_deref() == Some(thread)
        && session.cache.lock().backfill_current(generation)
}

/// Replays history newest page first; refuses to read while the epoch is not Ready.
pub(super) async fn replay<P: HistoryPort>(
    session: &Session,
    epoch: u64,
    generation: u64,
    thread: &str,
    port: &mut P,
) -> Result<Replayed, RuntimeFailure> {
    let mut cursor = None;
    let mut cursors = std::collections::HashSet::new();
    let mut history = RuntimeHistory::default();
    let mut answers = Vec::new();
    let mut emitted = 0;
    let mut replayed = Replayed { pages: 0, rows: 0 };
    loop {
        if !current(session, epoch, generation, thread) {
            return Err(RuntimeFailure::Superseded);
        }
        let RuntimeResult::HistoryPage {
            thread_id: read_thread,
            items,
            next_cursor,
            open_items,
            questions,
        } = port.read(cursor).await?
        else {
            return Err(RuntimeFailure::HookFailed(
                "History result is invalid".into(),
            ));
        };
        if read_thread != thread || !current(session, epoch, generation, thread) {
            return Err(RuntimeFailure::Superseded);
        }
        replayed.pages += 1;
        answers.extend(items.iter().filter(|item| item.kind == "user").cloned());
        history.reconcile_questions(&answers, questions);
        if !history.questions().is_empty() {
            port.questions(history.questions()).await?;
            if !current(session, epoch, generation, thread) {
                return Err(RuntimeFailure::Superseded);
            }
        }
        let mut rows = items;
        for open in &open_items {
            if !rows.iter().any(|row| row.id == open.item.id) {
                rows.push(open.item.clone());
            }
        }
        let landed = session
            .cache
            .lock()
            .reconcile_backfill(rows, replayed.pages > 1);
        replayed.rows += landed.len();
        adopt_open_turns(session, thread, &open_items);
        if emitted < EMITTED_TAIL && !landed.is_empty() {
            let take = landed.len().min(EMITTED_TAIL - emitted);
            port.emit(&landed[landed.len() - take..]);
            emitted += take;
        }
        port.page_landed(replayed.pages);
        let Some(next) = next_cursor else {
            return Ok(replayed);
        };
        if !cursors.insert(next.clone()) {
            return Err(RuntimeFailure::HookFailed("History cursor limit".into()));
        }
        cursor = Some(next);
    }
}

fn adopt_open_turns(session: &Session, thread: &str, open_items: &[OpenItem]) {
    for open in open_items {
        if session.completions.lock().contains(thread, &open.turn_id) {
            continue;
        }
        let mut context = session.context.lock();
        if context.active_turn_id.is_none() {
            context.active_turn_id = Some(open.turn_id.clone());
        }
    }
}

struct RpcPort {
    svc: ServiceContext,
    session: Arc<Session>,
    process: Arc<Process>,
    tab: String,
    thread: String,
    started: Instant,
    first_page_ms: Option<u128>,
}

impl HistoryPort for RpcPort {
    async fn read(&mut self, cursor: Option<String>) -> Result<RuntimeResult, RuntimeFailure> {
        let provider = self.session.provider.clone();
        super::run(
            &self.svc,
            &provider,
            &self.session,
            &self.process,
            RuntimeOperation::ReadHistory {
                thread_id: self.thread.clone(),
                cursor,
            },
        )
        .await
    }
    async fn questions(
        &mut self,
        questions: Vec<RuntimeHistoryQuestion>,
    ) -> Result<(), RuntimeFailure> {
        let owner = self.svc.clone();
        let target = self.tab.clone();
        let thread = self.thread.clone();
        let questions = tokio::task::spawn_blocking(move || {
            owner
                .pending_delivery_payloads
                .user_sends
                .persist_runtime_history_questions(&target, &thread, questions)
        })
        .await
        .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
        .map_err(RuntimeFailure::HookFailed)?;
        super::super::question_delivery::publish(&self.svc, &self.tab, &self.session, questions)
            .map_err(RuntimeFailure::HookFailed)
    }
    fn emit(&mut self, rows: &[ChatMsg]) {
        let context = self.session.context.lock().clone();
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            self.svc.ui.as_ref(),
            &self.tab,
            rows,
            None,
            context.thread_id.as_deref(),
            Some(&context.binding_nonce),
        );
    }
    fn page_landed(&mut self, ordinal: u32) {
        if ordinal == 1 {
            self.first_page_ms = Some(self.started.elapsed().as_millis());
        }
        crate::startup::boot_timing::mark(&self.tab, None, format!("history_page_{ordinal}"));
    }
}

pub(super) struct Reopen {
    pub tab: String,
    pub thread: String,
    pub epoch: u64,
    pub started: Instant,
    pub spawned_ms: Option<u128>,
    pub initialized_ms: u128,
    pub resumed_ms: u128,
    pub ready_ms: u128,
}

pub(super) fn start(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    generation: u64,
    reopen: Reopen,
) {
    let mut port = RpcPort {
        svc: svc.clone(),
        session: session.clone(),
        process: process.clone(),
        tab: reopen.tab.clone(),
        thread: reopen.thread.clone(),
        started: Instant::now(),
        first_page_ms: None,
    };
    let session = session.clone();
    crate::async_runtime::spawn(async move {
        let outcome = replay(
            &session,
            reopen.epoch,
            generation,
            &reopen.thread,
            &mut port,
        )
        .await;
        let tab = reopen.tab.as_str();
        let landed = match &outcome {
            Ok(_) => HistoryBackfill::Complete,
            Err(_) => HistoryBackfill::Failed,
        };
        if !session.cache.lock().finish_backfill(generation, landed) {
            return;
        }
        match outcome {
            Ok(replayed) => {
                crate::startup::boot_timing::mark(tab, None, "history_complete");
                log::debug!(
                    "[agent-runtime] tab={tab} reopen spawn={} init={}ms resume={}ms ready={}ms history={}ms first_page={}ms pages={} rows={}",
                    reopen
                        .spawned_ms
                        .map_or_else(|| "reused".to_string(), |ms| format!("{ms}ms")),
                    reopen.initialized_ms,
                    reopen.resumed_ms,
                    reopen.ready_ms,
                    reopen.started.elapsed().as_millis(),
                    port.first_page_ms.unwrap_or_default(),
                    replayed.pages,
                    replayed.rows,
                );
            }
            Err(RuntimeFailure::Superseded) => {
                log::debug!("[agent-runtime] tab={tab} history backfill superseded");
            }
            Err(reason) => {
                log::warn!("[agent-runtime] tab={tab} history backfill failed: {reason}");
            }
        }
        super::publish(&port.svc, tab, &session);
    });
}

#[cfg(test)]
#[path = "backfill/tests.rs"]
mod tests;
