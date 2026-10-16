use super::{RuntimePhase, Session};
use crate::service::{agent_runtime::model::HistoryBackfill, ServiceContext};
use codeterm_plugin::agent_runtime::*;
use codeterm_types::agent::chat_types::ChatMsg;
use std::{collections::HashSet, future::Future, sync::Arc, time::Instant};

use super::super::{model::RuntimeFailure, process::Process};

pub(super) trait HistoryPort {
    fn read(
        &mut self,
        cursor: Option<String>,
    ) -> impl Future<Output = Result<RuntimeResult, RuntimeFailure>> + Send;
    fn questions(
        &mut self,
        questions: Vec<RuntimeHistoryQuestion>,
    ) -> impl Future<Output = Result<(), RuntimeFailure>> + Send;
    fn page_landed(&mut self, _ordinal: u32) {}
}

/// History replay of one epoch: requested newest first; a plugin that answers ascending
/// is replayed whole before Ready through the append path.
pub(super) struct Replay {
    epoch: u64,
    generation: u64,
    thread: String,
    cursor: Option<String>,
    cursors: HashSet<String>,
    history: RuntimeHistory,
    answers: Vec<ChatMsg>,
    pub pages: u32,
    pub rows: usize,
    pub descending: bool,
    pub done: bool,
}

impl Replay {
    pub fn new(epoch: u64, generation: u64, thread: &str) -> Self {
        Self {
            epoch,
            generation,
            thread: thread.into(),
            cursor: None,
            cursors: HashSet::new(),
            history: RuntimeHistory::default(),
            answers: Vec::new(),
            pages: 0,
            rows: 0,
            descending: false,
            done: false,
        }
    }

    fn current(&self, session: &Session, ready: bool) -> bool {
        let (epoch, phase_ready) = {
            let state = session.state.lock();
            (state.epoch, matches!(state.phase, RuntimePhase::Ready))
        };
        let (context_epoch, thread) = {
            let context = session.context.lock();
            (context.epoch, context.thread_id.clone())
        };
        epoch == self.epoch
            && context_epoch == self.epoch
            && (!ready || phase_ready)
            && thread.as_deref() == Some(self.thread.as_str())
            && session.cache.lock().backfill_current(self.generation)
    }

    async fn page<P: HistoryPort>(
        &mut self,
        session: &Session,
        port: &mut P,
        ready: bool,
    ) -> Result<(), RuntimeFailure> {
        if !self.current(session, ready) {
            return Err(RuntimeFailure::Superseded);
        }
        let RuntimeResult::HistoryPage {
            thread_id,
            items,
            next_cursor,
            open_items,
            questions,
            order,
        } = port.read(self.cursor.clone()).await?
        else {
            return Err(RuntimeFailure::HookFailed(
                "History result is invalid".into(),
            ));
        };
        if thread_id != self.thread || !self.current(session, ready) {
            return Err(RuntimeFailure::Superseded);
        }
        let descending = order == Some(HistoryOrder::Desc);
        if self.pages == 0 {
            self.descending = descending;
        } else if descending != self.descending {
            return Err(RuntimeFailure::HookFailed(
                "History order changed between pages".into(),
            ));
        }
        self.pages += 1;
        self.answers
            .extend(items.iter().filter(|item| item.kind == "user").cloned());
        self.history.reconcile_questions(&self.answers, questions);
        if !self.history.questions().is_empty() {
            port.questions(self.history.questions()).await?;
            if !self.current(session, ready) {
                return Err(RuntimeFailure::Superseded);
            }
        }
        let mut rows = items;
        for open in &open_items {
            if !rows.iter().any(|row| row.id == open.item.id) {
                rows.push(open.item.clone());
            }
        }
        self.rows += if self.descending {
            session
                .cache
                .lock()
                .reconcile_backfill(rows, self.pages > 1)
        } else {
            let landed = rows.len();
            let mut cache = session.cache.lock();
            for row in rows {
                cache.reconcile(row);
            }
            landed
        };
        adopt_open_turns(session, &self.thread, &open_items);
        port.page_landed(self.pages);
        match next_cursor {
            None => self.done = true,
            Some(next) => {
                if !self.cursors.insert(next.clone()) {
                    return Err(RuntimeFailure::HookFailed("History cursor limit".into()));
                }
                self.cursor = Some(next);
            }
        }
        Ok(())
    }

    /// Lands the newest page before Ready so an open restored turn is adopted for steering.
    pub async fn first<P: HistoryPort>(
        &mut self,
        session: &Session,
        port: &mut P,
    ) -> Result<(), RuntimeFailure> {
        self.page(session, port, false).await?;
        while !self.descending && !self.done {
            self.page(session, port, false).await?;
        }
        Ok(())
    }

    /// Lands the older pages; refuses to read unless the epoch is Ready.
    pub async fn rest<P: HistoryPort>(
        &mut self,
        session: &Session,
        port: &mut P,
    ) -> Result<(), RuntimeFailure> {
        while !self.done {
            self.page(session, port, true).await?;
        }
        Ok(())
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

pub(super) struct RpcPort {
    svc: ServiceContext,
    session: Arc<Session>,
    process: Arc<Process>,
    tab: String,
    thread: String,
}

impl RpcPort {
    pub fn new(
        svc: &ServiceContext,
        session: &Arc<Session>,
        process: &Arc<Process>,
        tab: &str,
        thread: &str,
    ) -> Self {
        Self {
            svc: svc.clone(),
            session: session.clone(),
            process: process.clone(),
            tab: tab.into(),
            thread: thread.into(),
        }
    }
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
                order: Some(HistoryOrder::Desc),
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
    fn page_landed(&mut self, ordinal: u32) {
        crate::startup::boot_timing::mark(&self.tab, None, format!("history_page_{ordinal}"));
        super::publish(&self.svc, &self.tab, &self.session);
    }
}

pub(super) struct Reopen {
    pub started: Instant,
    pub spawned_ms: Option<u128>,
    pub initialized_ms: u128,
    pub resumed_ms: u128,
    pub first_page_ms: u128,
    pub ready_ms: u128,
}

/// Records the replay outcome once for its generation; a newer generation owns the state.
pub(super) fn finish(
    port: &RpcPort,
    replay: &Replay,
    outcome: Result<(), RuntimeFailure>,
    reopen: &Reopen,
) {
    let landed = match &outcome {
        Ok(()) => HistoryBackfill::Complete,
        Err(_) => HistoryBackfill::Failed,
    };
    if !port
        .session
        .cache
        .lock()
        .finish_backfill(replay.generation, landed)
    {
        return;
    }
    let tab = port.tab.as_str();
    match outcome {
        Ok(()) => {
            crate::startup::boot_timing::mark(tab, None, "history_complete");
            log::debug!(
                "[agent-runtime] tab={tab} reopen spawn={} init={}ms resume={}ms first_page={}ms ready={}ms history={}ms pages={} rows={}",
                reopen
                    .spawned_ms
                    .map_or_else(|| "reused".to_string(), |ms| format!("{ms}ms")),
                reopen.initialized_ms,
                reopen.resumed_ms,
                reopen.first_page_ms,
                reopen.ready_ms,
                reopen.started.elapsed().as_millis(),
                replay.pages,
                replay.rows,
            );
        }
        Err(RuntimeFailure::Superseded) => {
            log::debug!("[agent-runtime] tab={tab} history backfill superseded");
        }
        Err(reason) => {
            log::warn!("[agent-runtime] tab={tab} history backfill failed: {reason}");
        }
    }
    super::publish(&port.svc, tab, &port.session);
}

pub(super) fn continue_in_background(mut port: RpcPort, mut replay: Replay, reopen: Reopen) {
    crate::async_runtime::spawn(async move {
        let session = port.session.clone();
        let outcome = replay.rest(&session, &mut port).await;
        finish(&port, &replay, outcome, &reopen);
    });
}

#[cfg(test)]
#[path = "backfill/tests.rs"]
mod tests;
