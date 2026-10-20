use std::sync::Arc;

use parking_lot::Mutex;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};
use windows_sys::Win32::System::IO::PostQueuedCompletionStatus;

use super::{Cancellation, Registration, Wake};
use crate::probe::capture::{last_error, OwnedHandle};

pub(in crate::probe) const CANCEL_KEY: usize = 4;

struct State {
    event: OwnedHandle,
    port: Option<usize>,
    cancelled: bool,
    posted: bool,
    error: Option<String>,
}

struct EventWake {
    state: Mutex<State>,
}

// Windows event/port handles are transferred only under this mutex.
unsafe impl Send for State {}

impl State {
    fn post(&mut self) {
        if self.cancelled && !self.posted {
            if let Some(port) = self.port {
                self.posted = true;
                if unsafe {
                    PostQueuedCompletionStatus(port as HANDLE, 0, CANCEL_KEY, std::ptr::null())
                } == 0
                {
                    self.error = Some(last_error());
                }
            }
        }
    }
}

impl Wake for EventWake {
    fn wake(&self) {
        let mut state = self.state.lock();
        state.cancelled = true;
        if let Some(event) = state.event.raw() {
            if unsafe { SetEvent(event) } == 0 {
                state.error = Some(last_error());
            }
        }
        state.post();
    }
}

pub struct Events {
    wake: Arc<EventWake>,
    registration: Option<Registration>,
}

impl Events {
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::probe) fn fixture_argv(&self) -> Option<Arc<[String]>> {
        self.registration
            .as_ref()
            .and_then(Registration::fixture_argv)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::probe) fn waiting(&self, phase: super::WaitPhase) {
        if let Some(registration) = &self.registration {
            registration.waiting(phase);
        }
    }

    pub(in crate::probe) fn new(signal: &Cancellation) -> Result<Self, String> {
        let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        let event = OwnedHandle::new(raw).ok_or_else(last_error)?;
        let wake = Arc::new(EventWake {
            state: Mutex::new(State {
                event,
                port: None,
                cancelled: false,
                posted: false,
                error: None,
            }),
        });
        let registration = Some(signal.register(wake.clone()).map_err(str::to_string)?);
        Ok(Self { wake, registration })
    }

    pub(in crate::probe) fn attach_port(&self, port: HANDLE) {
        let mut state = self.wake.state.lock();
        state.port = Some(port as usize);
        state.post();
    }

    pub(in crate::probe) fn detach_port(&self) {
        self.wake.state.lock().port = None;
    }

    pub(in crate::probe) fn event(&self) -> Option<HANDLE> {
        self.wake.state.lock().event.raw()
    }

    pub(in crate::probe) fn cancelled(&self) -> bool {
        self.wake.state.lock().cancelled
    }

    pub(in crate::probe) fn take_delivery_error(&self) -> Option<String> {
        self.wake.state.lock().error.take()
    }

    pub(in crate::probe) fn close(&mut self) -> Result<(), String> {
        self.registration.take();
        let mut state = self.wake.state.lock();
        state.port = None;
        state.event.close()?;
        Ok(())
    }
}
