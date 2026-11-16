use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use uuid::Uuid;

pub const HIGH_WATERMARK: u64 = 256 * 1024;
pub const LOW_WATERMARK: u64 = 64 * 1024;
pub const ACK_FAILURE_BOUND: Duration = Duration::from_secs(10);

pub fn renderer_may_pause(
    booting: bool,
    has_agent: bool,
    stage: Option<codeterm_types::pty::BootStage>,
) -> bool {
    use codeterm_types::pty::BootStage;
    !booting
        && if has_agent {
            stage == Some(BootStage::AgentReady)
        } else {
            matches!(
                stage,
                None | Some(
                    BootStage::ShellReady | BootStage::PtyReattached | BootStage::AgentReady
                )
            )
        }
}

#[derive(Default)]
pub struct OutputFlow {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Default)]
struct State {
    generation: u64,
    produced: u64,
    owner: Option<Owner>,
    suspended: usize,
}

struct Owner {
    view: Uuid,
    attachment: Uuid,
    acknowledged: u64,
    paused: bool,
    deadline: Option<Instant>,
}

impl OutputFlow {
    pub fn bind(&self, generation: u64) {
        let mut state = self.state.lock();
        if state.generation != generation {
            let suspended = state.suspended;
            *state = State {
                generation,
                suspended,
                ..State::default()
            };
            self.changed.notify_waiters();
        }
    }

    pub fn produced(&self, generation: u64, total: u64) {
        let mut state = self.state.lock();
        if state.generation == generation {
            state.produced = state.produced.max(total);
        }
    }

    pub fn attach(
        &self,
        generation: u64,
        view: Uuid,
        attachment: Uuid,
        offset: u64,
        _now: Instant,
    ) -> bool {
        let mut state = self.state.lock();
        if generation != state.generation
            || offset > state.produced
            || attachment.is_nil()
            || view.is_nil()
        {
            return false;
        }
        if state.owner.is_some() {
            return false;
        }
        state.owner = Some(Owner {
            view,
            attachment,
            acknowledged: offset,
            paused: false,
            deadline: None,
        });
        self.changed.notify_waiters();
        true
    }

    pub fn acknowledge(&self, view: Uuid, attachment: Uuid, offset: u64, now: Instant) -> bool {
        let mut state = self.state.lock();
        if offset > state.produced {
            return false;
        }
        let Some(owner) = state.owner.as_mut() else {
            return false;
        };
        if owner.view != view || owner.attachment != attachment || offset <= owner.acknowledged {
            return false;
        }
        owner.acknowledged = offset;
        owner.deadline = owner.deadline.map(|_| now + ACK_FAILURE_BOUND);
        self.changed.notify_waiters();
        true
    }

    pub fn detach(&self, view: Uuid, attachment: Option<Uuid>) {
        let mut state = self.state.lock();
        if state.owner.as_ref().is_some_and(|owner| {
            owner.view == view && attachment.is_none_or(|id| id == owner.attachment)
        }) {
            state.owner = None;
            self.changed.notify_waiters();
        }
    }

    pub fn revoke(&self) {
        self.state.lock().owner = None;
        self.changed.notify_waiters();
    }

    pub fn suspend(self: &Arc<Self>) -> ReadPermit {
        self.state.lock().suspended += 1;
        self.changed.notify_waiters();
        ReadPermit(self.clone())
    }

    pub fn pause_deadline(&self, now: Instant) -> Option<Instant> {
        let mut state = self.state.lock();
        if state.suspended > 0 {
            return None;
        }
        let produced = state.produced;
        let owner = state.owner.as_mut()?;
        let pending = produced.saturating_sub(owner.acknowledged);
        owner.paused = pending >= HIGH_WATERMARK || (owner.paused && pending > LOW_WATERMARK);
        if !owner.paused {
            owner.deadline = None;
            return None;
        }
        let deadline = *owner.deadline.get_or_insert(now + ACK_FAILURE_BOUND);
        if now >= deadline {
            state.owner = None;
            return None;
        }
        Some(deadline)
    }

    pub async fn wait_to_read(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let Some(deadline) = self.pause_deadline(Instant::now()) else {
                return;
            };
            let _ =
                tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), changed).await;
        }
    }
}

pub struct ReadPermit(Arc<OutputFlow>);

impl Drop for ReadPermit {
    fn drop(&mut self) {
        let mut state = self.0.state.lock();
        state.suspended = state.suspended.saturating_sub(1);
        self.0.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_tests;
