use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use codeterm_plugin::converter::ConversionCancellation;
use parking_lot::Mutex;

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub trait Wake: Send + Sync {
    fn wake(&self);
}

#[cfg(test)]
type ReadinessHook = Arc<dyn Fn() -> Result<super::process::Deadlines, String> + Send + Sync>;

#[derive(Default)]
struct State {
    cancelled: bool,
    #[cfg(test)]
    readiness: Option<ReadinessHook>,
    wake: Option<Weak<dyn Wake>>,
    #[cfg(test)]
    wait_observer: Option<std::sync::mpsc::Sender<WaitPhase>>,
    #[cfg(all(test, windows))]
    fixture_argv: Option<Arc<[String]>>,
}

#[derive(Clone, Default)]
pub struct Cancellation {
    state: Arc<Mutex<State>>,
    conversion: ConversionCancellation,
}

impl Cancellation {
    #[cfg(all(test, windows))]
    pub fn for_windows_fixture(argv: Vec<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                fixture_argv: Some(argv.into()),
                ..State::default()
            })),
            conversion: ConversionCancellation::default(),
        }
    }

    pub fn new(cancelled: bool) -> Self {
        let cancellation = Self {
            state: Arc::new(Mutex::new(State {
                cancelled,
                ..State::default()
            })),
            conversion: ConversionCancellation::default(),
        };
        if cancelled {
            cancellation.conversion.cancel();
        }
        cancellation
    }

    pub fn load(&self, _: Ordering) -> bool {
        self.state.lock().cancelled
    }

    pub fn store(&self, cancelled: bool, _: Ordering) {
        if cancelled {
            self.cancel();
        }
    }

    pub fn cancel(&self) {
        self.conversion.cancel();
        let mut state = self.state.lock();
        if state.cancelled {
            return;
        }
        state.cancelled = true;
        if let Some(wake) = state.wake.as_ref().and_then(Weak::upgrade) {
            wake.wake();
        }
    }

    pub fn conversion_token(&self) -> ConversionCancellation {
        self.conversion.clone()
    }

    pub fn register(
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

pub struct Registration {
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
pub enum WaitPhase {
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
    pub fn with_readiness(
        ready: impl Fn() -> Result<super::process::Deadlines, String> + Send + Sync + 'static,
    ) -> Self {
        let signal = Self::default();
        signal.set_readiness(ready);
        signal
    }

    pub fn set_readiness(
        &self,
        ready: impl Fn() -> Result<super::process::Deadlines, String> + Send + Sync + 'static,
    ) {
        self.state.lock().readiness = Some(Arc::new(ready));
    }

    pub fn setup_deadlines(
        &self,
        deadlines: super::process::Deadlines,
    ) -> super::process::Deadlines {
        if self.state.lock().readiness.is_some() {
            super::process::Deadlines::start(
                super::test_readiness::SETUP_DEADLINE,
                super::test_readiness::SETUP_DEADLINE,
            )
            .unwrap()
        } else {
            deadlines
        }
    }

    pub fn child_ready(
        &self,
        deadlines: super::process::Deadlines,
    ) -> Result<super::process::Deadlines, String> {
        let readiness = self.state.lock().readiness.clone();
        match readiness {
            Some(ready) => ready(),
            None => Ok(deadlines),
        }
    }

    pub fn observe_waits(&self) -> std::sync::mpsc::Receiver<WaitPhase> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.state.lock().wait_observer = Some(tx);
        rx
    }
}

#[cfg(test)]
impl Registration {
    #[cfg(windows)]
    fn fixture_argv(&self) -> Option<Arc<[String]>> {
        self.signal.state.lock().fixture_argv.clone()
    }

    fn waiting(&self, phase: WaitPhase) {
        if let Some(observer) = &self.signal.state.lock().wait_observer {
            let _ = observer.send(phase);
        }
    }
}
