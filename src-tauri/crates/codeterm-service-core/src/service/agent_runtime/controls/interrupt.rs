use super::super::{
    engine,
    model::{RuntimeFailure, RuntimePhase},
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeOperation, RuntimeResult};
use std::{collections::HashSet, sync::Arc, time::Duration};

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
    turns: Vec<(String, String)>,
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
            .filter(|turn| threads.contains(&turn.thread_id))
            .map(|turn| (turn.thread_id.clone(), turn.turn_id.clone()))
            .collect::<Vec<_>>();
        if let Some(turn) = &context.active_turn_id {
            let root = (thread.clone(), turn.clone());
            if !turns.contains(&root) {
                turns.push(root);
            }
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
        if self
            .turns
            .iter()
            .all(|(thread, turn)| completed(thread, turn))
        {
            CompletionState::Complete
        } else {
            CompletionState::Waiting
        }
    }
}

pub(super) fn interrupt(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    tauri::async_runtime::block_on(async {
        let session = engine::ensure(svc, tab).await?;
        let permit = session
            .gate
            .try_acquire()
            .map_err(|_| RuntimeFailure::Refused("Runtime operation already in progress".into()))?;
        engine::validate_binding(svc, &session)?;
        let context = session.context.lock().clone();
        let target = InterruptTarget::capture(&context)?;
        let mut changes = session.control_changes.subscribe();
        let process = session
            .process
            .lock()
            .clone()
            .ok_or(RuntimeFailure::PipeEof)?;
        let mut ended = process.ended.clone();
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
        drop(permit);
        wait(
            svc,
            &session,
            &target,
            &mut changes,
            &mut ended,
            Duration::from_secs(30),
        )
        .await
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
                CompletionState::Complete => return Ok(()),
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
            let reason = RuntimeFailure::Refused(
                "Interrupt completion deadline exceeded; runtime recovery required".into(),
            );
            if engine::validate_binding(svc, session).is_ok()
                && session.context.lock().epoch == target.epoch
            {
                session.state.lock().degrade(target.epoch, &reason);
                let tab = session.context.lock().tab_id.clone();
                engine::publish(svc, &tab, session);
                svc.emit_state_changed();
            }
            Err(reason)
        }
    }
}

#[cfg(test)]
mod tests;
