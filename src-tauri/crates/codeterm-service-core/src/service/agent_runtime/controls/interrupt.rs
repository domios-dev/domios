use super::super::{
    engine,
    model::{RuntimeFailure, RuntimePhase},
};
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RuntimeActiveTurn, RuntimeContext, RuntimeOperation, RuntimeResult,
};
use parking_lot::Mutex;
use std::{
    collections::HashSet,
    sync::{Arc, LazyLock},
    time::Duration,
};

#[derive(Debug, PartialEq, Eq)]
enum CompletionState {
    Waiting,
    Complete,
    Superseded,
    Unloaded,
}

struct InterruptTarget {
    epoch: u64,
    nonce: String,
    thread: String,
    turns: Vec<RuntimeActiveTurn>,
}

impl InterruptTarget {
    fn capture(context: &RuntimeContext) -> Result<Self, RuntimeFailure> {
        let thread = context
            .thread_id
            .clone()
            .ok_or(RuntimeFailure::Superseded)?;
        let mut threads = HashSet::from([thread.clone()]);
        loop {
            let before = threads.len();
            for parent in &context.thread_parents {
                if threads.contains(&parent.parent_thread_id) {
                    threads.insert(parent.thread_id.clone());
                }
            }
            if before == threads.len() {
                break;
            }
        }
        let mut turns = context
            .active_turns
            .iter()
            .filter(|turn| threads.contains(&turn.thread_id) && turn.thread_id != thread)
            .cloned()
            .collect::<Vec<_>>();
        let root = context
            .active_turn_id
            .as_ref()
            .map(|turn_id| RuntimeActiveTurn {
                thread_id: thread.clone(),
                turn_id: turn_id.clone(),
                parent_thread_id: None,
            })
            .or_else(|| {
                context
                    .active_turns
                    .iter()
                    .find(|turn| turn.thread_id == thread)
                    .cloned()
            });
        if let Some(root) = root {
            turns.push(root);
        }
        Ok(Self {
            epoch: context.epoch,
            nonce: context.binding_nonce.clone(),
            thread,
            turns,
        })
    }

    fn state(
        &self,
        context: &RuntimeContext,
        phase: &RuntimePhase,
        completed: impl Fn(&str, &str) -> bool,
    ) -> CompletionState {
        if context.epoch != self.epoch
            || context.binding_nonce != self.nonce
            || context.thread_id.as_ref() != Some(&self.thread)
        {
            return CompletionState::Superseded;
        }
        if matches!(
            phase,
            RuntimePhase::Unloaded { .. } | RuntimePhase::Degraded { .. }
        ) {
            return CompletionState::Unloaded;
        }
        if self.turns.iter().all(|turn| {
            completed(&turn.thread_id, &turn.turn_id)
                && !(turn.thread_id == self.thread
                    && context.active_turn_id.as_ref() == Some(&turn.turn_id))
                && !context.active_turns.iter().any(|active| {
                    active.thread_id == turn.thread_id && active.turn_id == turn.turn_id
                })
        }) {
            CompletionState::Complete
        } else {
            CompletionState::Waiting
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InterruptOutcome {
    Stopped,
    NoTurn,
    /// An earlier stop or runtime operation still owns the outcome; this request sent nothing.
    Pending,
    Dropped(InterruptDropped),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterruptDropped {
    TurnEnded,
    TurnChanged,
}

/// The work a Stop press meant to end. The gate can free long after the press, and a late
/// interrupt may only reach this work.
#[derive(Debug, Default)]
struct StopPress {
    session: Option<(u64, String, Option<String>)>,
    turns: HashSet<String>,
    sends: HashSet<String>,
}

impl StopPress {
    fn of(svc: &ServiceContext, session: &engine::Session, context: &RuntimeContext) -> Self {
        let mut turns = super::super::turn_requests::open_turns(session, context)
            .into_iter()
            .collect::<HashSet<_>>();
        turns.extend(context.active_turn_id.clone());
        turns.extend(context.active_turns.iter().map(|turn| turn.turn_id.clone()));
        Self {
            session: Some((
                context.epoch,
                context.binding_nonce.clone(),
                context.thread_id.clone(),
            )),
            turns,
            sends: outstanding_claims(svc, context),
        }
    }

    /// A runtime still loading has not reported its turns, so that press binds to nothing.
    fn at_press(svc: &ServiceContext, tab: &str) -> Option<Self> {
        let session = engine::for_tab(tab)?;
        if !matches!(session.state.lock().phase, RuntimePhase::Ready) {
            return None;
        }
        let context = session.context.lock().clone();
        Some(Self::of(svc, &session, &context))
    }

    fn dropped(&self, now: &Self, active: Option<&str>) -> Option<InterruptDropped> {
        let same_work = if self.turns.is_empty() && self.sends.is_empty() {
            now.turns.is_empty() && now.sends.is_empty()
        } else if self.turns.is_empty() {
            !self.sends.is_disjoint(&now.sends)
        } else {
            match active {
                Some(active) => self.turns.contains(active),
                None => !self.turns.is_disjoint(&now.turns),
            }
        };
        if self.session == now.session && same_work {
            None
        } else if now.turns.is_empty() && now.sends.is_empty() {
            Some(InterruptDropped::TurnEnded)
        } else {
            Some(InterruptDropped::TurnChanged)
        }
    }
}

static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// One interrupt per tab, so a repeated Stop never queues behind the first and hits a later turn.
struct InterruptClaim(String);

impl InterruptClaim {
    fn take(tab: &str) -> Option<Self> {
        IN_FLIGHT
            .lock()
            .insert(tab.to_owned())
            .then(|| Self(tab.to_owned()))
    }
}

impl Drop for InterruptClaim {
    fn drop(&mut self) {
        IN_FLIGHT.lock().remove(&self.0);
    }
}

pub(super) fn interrupt(svc: &ServiceContext, tab: &str) -> Result<InterruptOutcome, String> {
    let Some(_claim) = InterruptClaim::take(tab) else {
        return Ok(InterruptOutcome::Pending);
    };
    interrupt_pressed(svc, tab, StopPress::at_press(svc, tab))
}

fn interrupt_pressed(
    svc: &ServiceContext,
    tab: &str,
    press: Option<StopPress>,
) -> Result<InterruptOutcome, String> {
    super::super::capacity_retry::cancel(svc, tab)?;
    crate::async_runtime::block_on(async {
        let session = engine::ensure(svc, tab).await?;
        let Ok(permit) = tokio::time::timeout(super::GATE_WAIT, session.gate.acquire()).await
        else {
            return Ok(InterruptOutcome::Pending);
        };
        let permit = permit.map_err(|_| RuntimeFailure::Superseded)?;
        engine::validate_binding(svc, &session)?;
        let context = session.context.lock().clone();
        if let Some(reason) = press.as_ref().and_then(|press| {
            press.dropped(
                &StopPress::of(svc, &session, &context),
                context.active_turn_id.as_deref(),
            )
        }) {
            log::info!("[tab-stop] interrupt dropped tab={tab} reason={reason:?}");
            return Ok(InterruptOutcome::Dropped(reason));
        }
        let mut target = InterruptTarget::capture(&context)?;
        let no_provider_turn =
            super::super::turn_requests::no_provider_turn(&session, context.epoch);
        if no_provider_turn {
            target.turns.clear();
        }
        let before_first = before_first_turn(svc, &session, &context);
        let had_turn = !target.turns.is_empty() || before_first;
        let process = session
            .process
            .lock()
            .clone()
            .ok_or(RuntimeFailure::PipeEof)?;
        super::super::client_bridge::cancel_all(svc, &session, &process, false).await?;
        let context = if had_turn {
            context
        } else {
            super::super::subscriber::publish_events(
                svc,
                &session,
                context.epoch,
                vec![
                    codeterm_plugin::agent_runtime::AgentRuntimeEvent::TurnState {
                        thread_id: target.thread.clone(),
                        turn_id: None,
                        parent_thread_id: None,
                        state: codeterm_plugin::agent_runtime::RuntimeTurnState::Idle,
                        detail: None,
                        status_line: None,
                        started_at: None,
                        completed_at: None,
                    },
                ],
                chrono::Utc::now().timestamp_millis().max(1) as u64,
            )
            .await?;
            if no_provider_turn {
                session.context.lock().active_turns.clear();
            }
            if !super::super::background::supported(&session) {
                return Ok(InterruptOutcome::NoTurn);
            }
            session.context.lock().clone()
        };
        {
            let mut current = session.context.lock();
            if current.epoch != context.epoch
                || current.binding_nonce != context.binding_nonce
                || current.thread_id != context.thread_id
                || current.active_turn_id != context.active_turn_id
                || current.active_turns != context.active_turns
            {
                return Err(RuntimeFailure::Superseded);
            }
            current.active_turns = target.turns.clone();
        }
        let mut changes = session.control_changes.subscribe();
        let mut ended = process.ended.clone();
        let background = super::super::background::supported(&session);
        let captured = if background {
            match process.observe().await {
                Ok(captured) => captured,
                Err(reason) => {
                    log::debug!(
                        "[agent-runtime] initial interrupt process observation failed: {reason}"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let interrupted = async {
            if target.turns.is_empty() && background {
                return Ok(());
            }
            let result = engine::run(
                svc,
                &session.provider,
                &session,
                &process,
                RuntimeOperation::Interrupt {
                    thread_id: target.thread.clone(),
                    active_turn_id: context.active_turn_id,
                },
            )
            .await?;
            if !matches!(result, RuntimeResult::Done) {
                return Err(RuntimeFailure::Refused(
                    "Runtime interrupt was not accepted".into(),
                ));
            }
            wait(
                svc,
                &session,
                &target,
                &mut changes,
                &mut ended,
                Duration::from_secs(30),
            )
            .await
        }
        .await;
        if background {
            if let Err(reason) = super::super::background::cleanup(
                svc,
                &session,
                &process,
                super::super::background::Mode::Interrupt,
                captured,
                true,
            )
            .await
            {
                if engine::validate_binding(svc, &session).is_ok()
                    && session.context.lock().epoch == target.epoch
                {
                    session.state.lock().degrade(target.epoch, &reason);
                    engine::publish_degraded(svc, tab, &session);
                    svc.emit_state_changed(MuxEmitCause::RuntimeSignal);
                }
                return Err(reason);
            }
        }
        drop(permit);
        let outcome = if had_turn {
            InterruptOutcome::Stopped
        } else {
            InterruptOutcome::NoTurn
        };
        if background
            && process.terminated()
            && engine::validate_binding(svc, &session).is_ok()
            && session.context.lock().epoch == target.epoch
        {
            return Ok(outcome);
        }
        interrupted.map(|()| outcome)
    })
    .map_err(|reason: RuntimeFailure| reason.to_string())
}

async fn wait(
    svc: &ServiceContext,
    session: &Arc<engine::Session>,
    target: &InterruptTarget,
    changes: &mut tokio::sync::watch::Receiver<()>,
    ended: &mut tokio::sync::watch::Receiver<Option<RuntimeFailure>>,
    deadline: Duration,
) -> Result<(), RuntimeFailure> {
    let result = tokio::time::timeout(deadline, async {
        loop {
            changes.borrow_and_update();
            engine::validate_binding(svc, session)?;
            let context = session.context.lock().clone();
            let phase = session.state.lock().phase.clone();
            let state = target.state(&context, &phase, |thread, turn| {
                session.completions.lock().contains(thread, turn)
            });
            match state {
                CompletionState::Complete
                    if !target.turns.is_empty()
                        || !super::super::turn_requests::waiting(session, target.epoch) =>
                {
                    return Ok(())
                }
                CompletionState::Complete => {}
                CompletionState::Superseded => return Err(RuntimeFailure::Superseded),
                CompletionState::Unloaded => {
                    return Err(RuntimeFailure::Refused(
                        "Runtime unloaded before interrupt completion".into(),
                    ))
                }
                CompletionState::Waiting => {}
            }
            if let Some(reason) = ended.borrow_and_update().clone() {
                return Err(reason);
            }
            tokio::select! {
                changed = changes.changed() => { changed.map_err(|_| RuntimeFailure::Superseded)?; }
                changed = ended.changed() => { changed.map_err(|_| RuntimeFailure::PipeEof)?; }
            }
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            if super::super::turn_requests::cancel_timed_out(svc, session, target.epoch).await? {
                return Ok(());
            }
            // A provider that stays silent has refused nothing, and a refusal would bar every later resume.
            let reason = RuntimeFailure::Deadline;
            if engine::validate_binding(svc, session).is_ok()
                && session.context.lock().epoch == target.epoch
            {
                session.state.lock().degrade(target.epoch, &reason);
                let tab = session.context.lock().tab_id.clone();
                engine::publish_degraded(svc, &tab, session);
                svc.emit_state_changed(MuxEmitCause::RuntimeSignal);
            }
            Err(reason)
        }
    }
}

#[cfg(test)]
mod tests;

fn before_first_turn(
    svc: &ServiceContext,
    session: &engine::Session,
    context: &RuntimeContext,
) -> bool {
    let current = session.context.lock().clone();
    if current.tab_id != context.tab_id
        || current.thread_id != context.thread_id
        || current.epoch != context.epoch
        || current.binding_nonce != context.binding_nonce
    {
        return false;
    }
    let declared = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime())
        .and_then(|cap| cap.interrupt)
        .is_some_and(|interrupt| interrupt.before_first_turn);
    declared
        && context.active_turn_id.is_none()
        && (super::super::turn_requests::written_pending(session, context)
            || !outstanding_claims(svc, context).is_empty())
}

fn outstanding_claims(svc: &ServiceContext, context: &RuntimeContext) -> HashSet<String> {
    context
        .claimed_sends
        .iter()
        .filter(|claim| {
            Some(&claim.thread_id) == context.thread_id.as_ref()
                && svc.chat_send_ledger.has_rpc_handoff(&claim.send_id)
                && svc
                    .chat_send_ledger
                    .status(&claim.send_id)
                    .is_some_and(|status| {
                        status.tab_id == context.tab_id
                            && status.state
                                == crate::service::chat_send_transaction::ChatSendState::Pending
                    })
        })
        .map(|claim| claim.send_id.clone())
        .collect()
}

pub(crate) fn before_first_turn_pending(svc: &ServiceContext, tab: &str) -> bool {
    let Some(session) = engine::for_tab(tab) else {
        return false;
    };
    if engine::validate_binding(svc, &session).is_err() {
        return false;
    }
    let context = session.context.lock().clone();
    before_first_turn(svc, &session, &context)
}
