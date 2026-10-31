use codeterm_neighbors::{Error, Result};
use std::{
    future::Future,
    sync::{Arc, Weak},
};
use tokio::sync::{mpsc, oneshot};

const CAPACITY: usize = 32;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Kind {
    Projection,
    Activation,
    Retirement,
    Removal { friendship: String, generation: u64 },
    Selection,
    StopLocal,
}

impl Kind {
    fn candidate(&self) -> bool {
        matches!(self, Self::Projection | Self::Activation)
    }
}

struct State {
    revision: Arc<()>,
    pending: usize,
    controls: usize,
    active: bool,
    closed: bool,
    off_pending: bool,
    error: Option<Error>,
    recovery: Option<(Kind, Error)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            revision: Arc::new(()),
            pending: 0,
            controls: 0,
            active: false,
            closed: false,
            off_pending: false,
            error: None,
            recovery: None,
        }
    }
}

impl State {
    fn error(&self) -> Option<Error> {
        self.recovery
            .as_ref()
            .map(|(_, error)| error.clone())
            .or_else(|| self.error.clone())
    }

    fn admits_control(&self, kind: &Kind) -> bool {
        if self
            .recovery
            .as_ref()
            .is_some_and(|(failed, _)| failed != kind && *kind != Kind::Retirement)
        {
            return false;
        }
        *kind != Kind::Selection || self.error.is_none()
    }
}

struct Inner {
    state: parking_lot::Mutex<State>,
    sender: mpsc::Sender<Job>,
    #[cfg(test)]
    changed: tokio::sync::Notify,
}

pub(super) struct Coordinator(Arc<Inner>);

impl Default for Coordinator {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel(CAPACITY);
        let inner = Arc::new(Inner {
            state: Default::default(),
            sender,
            #[cfg(test)]
            changed: Default::default(),
        });
        crate::async_runtime::spawn(run(receiver, Arc::downgrade(&inner)));
        Self(inner)
    }
}

pub(super) struct Receipt<T>(oneshot::Receiver<Result<T>>);

impl<T> Receipt<T> {
    pub(super) async fn wait(self) -> Result<T> {
        self.0.await.map_err(|_| Error::Transport)?
    }
}

struct Job {
    revision: Arc<()>,
    kind: Kind,
    port: Box<dyn JobPort>,
}

#[async_trait::async_trait]
trait JobPort: Send {
    fn abandoned(&self) -> bool;
    fn reject(self: Box<Self>, error: Error) -> Completed;
    async fn execute(self: Box<Self>) -> Completed;
}

struct Completed {
    outcome: Result<()>,
    publish: Box<dyn FnOnce() + Send>,
}

struct Work<F, T> {
    work: F,
    reply: oneshot::Sender<Result<T>>,
}

#[async_trait::async_trait]
impl<F, R, T> JobPort for Work<F, T>
where
    F: FnOnce() -> R + Send + 'static,
    R: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    fn abandoned(&self) -> bool {
        self.reply.is_closed()
    }
    fn reject(self: Box<Self>, error: Error) -> Completed {
        Completed {
            outcome: Err(error.clone()),
            publish: Box::new(move || {
                let _ = self.reply.send(Err(error));
            }),
        }
    }
    async fn execute(self: Box<Self>) -> Completed {
        let Self { work, reply } = *self;
        let result = work().await;
        let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
        Completed {
            outcome,
            publish: Box::new(move || {
                let _ = reply.send(result);
            }),
        }
    }
}

impl Coordinator {
    pub(super) fn require_readable(&self) -> Result<()> {
        let state = self.0.state.lock();
        if state.closed || state.off_pending || state.controls > 0 || state.error().is_some() {
            return Err(state.error().unwrap_or(Error::Revoked));
        }
        Ok(())
    }
    pub(super) fn close(&self) {
        let mut state = self.0.state.lock();
        state.closed = true;
        state.revision = Arc::new(());
    }

    pub(super) fn stop_pending(&self) {
        let mut state = self.0.state.lock();
        state.off_pending = true;
        state.closed = true;
        state.revision = Arc::new(());
    }

    pub(super) fn submit<F, R, T>(&self, kind: Kind, work: F) -> Result<Receipt<T>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let mut state = self.0.state.lock();
        if kind != Kind::StopLocal && state.off_pending {
            return Err(state.error().unwrap_or(Error::Revoked));
        }
        if !kind.candidate() && !state.admits_control(&kind) {
            return Err(state.error().unwrap_or(Error::Transport));
        }
        if kind.candidate() && (state.closed || state.controls > 0 || state.error().is_some()) {
            return Err(state.error().unwrap_or(Error::Revoked));
        }
        let limit = if kind.candidate() {
            CAPACITY - 1
        } else {
            CAPACITY
        };
        if state.pending >= limit {
            if !kind.candidate() {
                state.closed = true;
                state.error = Some(Error::Transport);
            }
            return Err(Error::Transport);
        }
        let (reply, receiver) = oneshot::channel();
        let revision = if kind == Kind::Activation {
            state.revision.clone()
        } else {
            Arc::new(())
        };
        if self
            .0
            .sender
            .try_send(Job {
                revision: revision.clone(),
                kind: kind.clone(),
                port: Box::new(Work { work, reply }),
            })
            .is_err()
        {
            state.closed = true;
            state.error = Some(Error::Transport);
            return Err(Error::Transport);
        }
        state.revision = revision;
        state.pending += 1;
        if !kind.candidate() {
            state.controls += 1;
            state.closed = true;
        }
        Ok(Receipt(receiver))
    }

    #[cfg(test)]
    fn outcome(&self) -> (usize, bool, bool, Option<Error>) {
        let state = self.0.state.lock();
        (
            state.pending,
            state.active,
            state.closed || state.off_pending,
            state.error(),
        )
    }

    #[cfg(test)]
    pub(in crate::service::neighbors) async fn idle(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.0.state.lock().pending == 0 {
                return;
            }
            changed.await;
        }
    }
}

struct OwnerExit(Weak<Inner>);
impl Drop for OwnerExit {
    fn drop(&mut self) {
        if let Some(inner) = self.0.upgrade() {
            let mut state = inner.state.lock();
            state.closed = true;
            state.error = Some(Error::Transport);
        }
    }
}

async fn run(mut receiver: mpsc::Receiver<Job>, inner: Weak<Inner>) {
    let _exit = OwnerExit(inner.clone());
    while let Some(job) = receiver.recv().await {
        let Some(inner) = inner.upgrade() else {
            break;
        };
        let admitted = {
            let mut state = inner.state.lock();
            let admitted = if !job.kind.candidate() {
                (job.kind != Kind::Selection || !state.off_pending)
                    && state.admits_control(&job.kind)
            } else {
                !state.closed
                    && !state.off_pending
                    && state.error().is_none()
                    && Arc::ptr_eq(&state.revision, &job.revision)
                    && !job.port.abandoned()
            };
            state.active = admitted;
            admitted
        };
        let completed = if admitted {
            job.port.execute().await
        } else {
            job.port.reject(Error::Revoked)
        };
        let mut state = inner.state.lock();
        state.active = false;
        state.pending -= 1;
        if !job.kind.candidate() {
            state.controls -= 1;
            match &completed.outcome {
                Ok(()) => {
                    state.error = None;
                    if state
                        .recovery
                        .as_ref()
                        .is_some_and(|(failed, _)| *failed == job.kind)
                    {
                        state.recovery = None;
                    }
                    if job.kind == Kind::StopLocal {
                        state.off_pending = false;
                    }
                    if state.recovery.is_none() && Arc::ptr_eq(&state.revision, &job.revision) {
                        state.closed = false;
                    }
                }
                Err(error) => {
                    if admitted && state.recovery.is_none() {
                        state.recovery = Some((job.kind.clone(), error.clone()));
                    }
                    state.closed = true;
                }
            }
        }
        if let Err(error) = &completed.outcome {
            log::debug!("[neighbors] owned reconciliation failed: {error}");
        }
        drop(state);
        #[cfg(test)]
        inner.changed.notify_waiters();
        (completed.publish)();
    }
}

impl crate::service::ServiceContext {
    pub(super) fn neighbor_reconciliation(&self) -> &Coordinator {
        self.extensions.get_or_init(Coordinator::default)
    }
}

#[cfg(test)]
mod tests;
