use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

#[cfg(unix)]
pub(super) mod unix;
#[cfg(windows)]
pub(super) mod windows;

pub(in crate::sentinel) trait Wake: Send + Sync {
    fn wake(&self);
}

#[derive(Default)]
struct State {
    cancelled: bool,
    wake: Option<Weak<dyn Wake>>,
    #[cfg(test)]
    wait_observer: Option<std::sync::mpsc::Sender<WaitPhase>>,
}

#[derive(Clone, Default)]
pub(in crate::sentinel) struct Cancellation {
    state: Arc<Mutex<State>>,
}

impl Cancellation {
    pub(in crate::sentinel) fn new(cancelled: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                cancelled,
                ..State::default()
            })),
        }
    }

    pub(in crate::sentinel) fn load(&self, _: Ordering) -> bool {
        self.state.lock().cancelled
    }

    pub(in crate::sentinel) fn store(&self, cancelled: bool, _: Ordering) {
        if cancelled {
            self.cancel();
        }
    }

    pub(in crate::sentinel) fn cancel(&self) {
        let mut state = self.state.lock();
        if state.cancelled {
            return;
        }
        state.cancelled = true;
        if let Some(wake) = state.wake.as_ref().and_then(Weak::upgrade) {
            wake.wake();
        }
    }

    pub(in crate::sentinel) fn register(
        &self,
        wake: Arc<dyn Wake>,
    ) -> Result<Registration, &'static str> {
        let mut state = self.state.lock();
        if state.wake.as_ref().and_then(Weak::upgrade).is_some() {
            return Err("probe_cancel_registration_busy");
        }
        state.wake = Some(Arc::downgrade(&wake));
        if state.cancelled {
            wake.wake();
        }
        Ok(Registration {
            signal: self.clone(),
            wake,
        })
    }
}

pub(in crate::sentinel) struct Registration {
    signal: Cancellation,
    wake: Arc<dyn Wake>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut state = self.signal.state.lock();
        if state
            .wake
            .as_ref()
            .is_some_and(|wake| wake.ptr_eq(&Arc::downgrade(&self.wake)))
        {
            state.wake = None;
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::sentinel) enum WaitPhase {
    Capture,
    Parent,
    #[cfg(windows)]
    ParentAfterEof,
    #[cfg(windows)]
    WindowsIo {
        operation: &'static str,
        stdout: Option<bool>,
        bytes: Option<u32>,
        error: Option<u32>,
        pending: Option<bool>,
        eof: Option<bool>,
    },
    #[cfg(windows)]
    WindowsCapture {
        stdout_eof: bool,
        stderr_eof: bool,
        stdout_pending: bool,
        stderr_pending: bool,
        parent_exited: bool,
        cancel_attempted: bool,
    },
}

#[cfg(test)]
impl Cancellation {
    pub(in crate::sentinel) fn observe_waits(&self) -> std::sync::mpsc::Receiver<WaitPhase> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.state.lock().wait_observer = Some(tx);
        rx
    }
}

#[cfg(test)]
impl Registration {
    fn waiting(&self, phase: WaitPhase) {
        if let Some(observer) = &self.signal.state.lock().wait_observer {
            let _ = observer.send(phase);
        }
    }
}
