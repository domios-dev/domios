use super::*;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::watch;

pub(super) struct EpochStarts {
    nonce: String,
    epoch: u64,
    starts: Arc<TurnStarts>,
}

pub(crate) struct TurnStarts {
    seen: Mutex<VecDeque<(u64, String)>>,
    sequence: watch::Sender<u64>,
}

impl Default for TurnStarts {
    fn default() -> Self {
        Self {
            seen: Mutex::new(VecDeque::new()),
            sequence: watch::channel(0).0,
        }
    }
}

impl TurnStarts {
    pub(crate) fn sequence(&self) -> u64 {
        *self.sequence.borrow()
    }

    pub(crate) fn observe(&self, turn: &str) {
        let mut seen = self.seen.lock();
        if seen.iter().any(|(_, id)| id == turn) {
            return;
        }
        let next = self.sequence().saturating_add(1);
        seen.push_back((next, turn.into()));
        while seen.len() > 64 {
            seen.pop_front();
        }
        self.sequence.send_replace(next);
    }

    fn started(&self, before: u64, turn: Option<&str>) -> bool {
        self.seen.lock().iter().any(|(sequence, id)| match turn {
            Some(turn) => *sequence > before && id == turn,
            None => *sequence > before,
        })
    }

    async fn wait(&self, before: u64, turn: Option<&str>) {
        let mut changed = self.sequence.subscribe();
        loop {
            if self.started(before, turn) {
                return;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    }
}

impl ServiceContext {
    pub(crate) fn runtime_turn_starts(
        &self,
        tab: &str,
        nonce: &str,
        epoch: u64,
    ) -> Arc<TurnStarts> {
        let mut starts = self.ext::<Arc<TabReopenGuards>>().turn_starts.lock();
        let entry = starts.entry(tab.into()).or_insert_with(|| EpochStarts {
            nonce: nonce.into(),
            epoch,
            starts: Arc::new(TurnStarts::default()),
        });
        if entry.nonce != nonce || entry.epoch != epoch {
            *entry = EpochStarts {
                nonce: nonce.into(),
                epoch,
                starts: Arc::new(TurnStarts::default()),
            };
        }
        entry.starts.clone()
    }

    pub(crate) fn arm_runtime_turn_start(
        &self,
        session: &Arc<super::super::agent_runtime::engine::Session>,
        context: codeterm_plugin::agent_runtime::RuntimeContext,
        starts: Arc<TurnStarts>,
        before: u64,
        turn: Option<String>,
    ) {
        let svc = self.clone();
        let session = session.clone();
        crate::async_runtime::spawn(async move {
            if tokio::time::timeout(
                Duration::from_secs(60),
                starts.wait(before, turn.as_deref()),
            )
            .await
            .is_ok()
            {
                return;
            }
            if session.context.lock().epoch != context.epoch
                || super::super::agent_runtime::engine::validate_binding(&svc, &session).is_err()
            {
                return;
            }
            let reason = super::super::agent_runtime::model::RuntimeFailure::Deadline;
            session.state.lock().degrade(context.epoch, &reason);
            super::super::agent_runtime::engine::publish(&svc, &context.tab_id, &session);
            svc.emit_state_changed(super::super::MuxEmitCause::RuntimeSignal);
            svc.chatbackend_turn_start_incident(
                &context.tab_id,
                &context.binding_nonce,
                context.epoch,
            );
        });
    }
}

#[cfg(test)]
mod tests;
