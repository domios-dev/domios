use super::{
    last_error_code, run_listener, PostThreadMessageW, SelectedInputIdentity, UnavailableReason,
    WaitForSingleObject, WindowsInputEvent, LISTENER_CLAIMED, WM_QUIT,
};
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SETUP_DEADLINE: Duration = Duration::from_secs(5);
const TEARDOWN_DEADLINE: Duration = Duration::from_secs(5);
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const WAIT_FAILED: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service::local_recording::platform) enum SetupFailure {
    Unavailable(UnavailableReason),
    Stopped,
    Timeout,
    CompletionLost,
}

pub(in crate::service::local_recording::platform) struct WindowsInputPendingSubscription {
    control: Arc<ThreadControl>,
    startup: Option<Receiver<Result<(), SetupFailure>>>,
    completion: Option<Receiver<Result<(), TeardownError>>>,
    thread: Option<JoinHandle<()>>,
    abandoned: bool,
}

pub(in crate::service::local_recording::platform) struct WindowsInputSubscription {
    control: Arc<ThreadControl>,
    completion: Option<Receiver<Result<(), TeardownError>>>,
    thread: Option<JoinHandle<()>>,
    stop_requested: bool,
}

pub(in crate::service::local_recording::platform) struct WindowsInputSetupFailure {
    pub reason: SetupFailure,
    pub teardown: WindowsInputTeardown,
}

pub(in crate::service::local_recording::platform) struct WindowsInputTeardown {
    completion: Option<Receiver<Result<(), TeardownError>>>,
    thread: Option<JoinHandle<()>>,
    completion_result: Option<Result<(), TeardownError>>,
    wake_error: Option<u32>,
    joined: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service::local_recording::platform) enum TeardownError {
    CompletionLost,
    CompletionTimeout,
    ThreadExitTimeout,
    ThreadWaitFailed(u32),
    ThreadPanicked,
    RawInputUnregister(u32),
    WindowDestroy(u32),
}

impl WindowsInputSubscription {
    pub(in crate::service::local_recording::platform) fn start_pending(
        identity: SelectedInputIdentity,
        generation: u64,
        callback: impl Fn(WindowsInputEvent) + Send + Sync + 'static,
    ) -> Result<WindowsInputPendingSubscription, String> {
        if !identity.is_supported_collection() {
            return Err("selected input identity is not the supported LARK Col01 path".into());
        }
        LISTENER_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "a LARK Raw Input subscription is already active".to_string())?;
        let control = Arc::new(ThreadControl::default());
        let (startup_tx, startup_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();
        let callback: Arc<dyn Fn(WindowsInputEvent) + Send + Sync> = Arc::new(callback);
        let thread_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name("lark-selected-raw-input".into())
            .spawn(move || {
                super::run_listener(
                    thread_control,
                    identity,
                    generation,
                    callback,
                    startup_tx,
                    completion_tx,
                )
            })
            .map_err(|error| {
                LISTENER_CLAIMED.store(false, Ordering::Release);
                format!("could not start LARK input thread: {error}")
            })?;
        Ok(WindowsInputPendingSubscription {
            control,
            startup: Some(startup_rx),
            completion: Some(completion_rx),
            thread: Some(thread),
            abandoned: false,
        })
    }

    pub(in crate::service::local_recording::platform) fn request_stop(
        mut self,
    ) -> WindowsInputTeardown {
        self.stop_requested = true;
        WindowsInputTeardown::stopping(&self.control, self.completion.take(), self.thread.take())
    }
}

impl Drop for WindowsInputSubscription {
    fn drop(&mut self) {
        if !self.stop_requested {
            self.control.request_stop();
        }
    }
}

impl WindowsInputPendingSubscription {
    pub(in crate::service::local_recording::platform) fn wait_until_ready(
        mut self,
    ) -> Result<WindowsInputSubscription, WindowsInputSetupFailure> {
        let result = self
            .startup
            .as_ref()
            .expect("startup receiver")
            .recv_timeout(SETUP_DEADLINE);
        match result {
            Ok(Ok(())) => {
                self.startup.take();
                self.abandoned = true;
                Ok(WindowsInputSubscription {
                    control: Arc::clone(&self.control),
                    completion: self.completion.take(),
                    thread: self.thread.take(),
                    stop_requested: false,
                })
            }
            Ok(Err(reason)) => Err(self.fail_setup(reason)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(self.fail_setup(SetupFailure::Timeout)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(self.fail_setup(SetupFailure::CompletionLost))
            }
        }
    }

    pub(in crate::service::local_recording::platform) fn request_stop(
        self,
    ) -> WindowsInputTeardown {
        self.into_teardown()
    }

    fn fail_setup(self, reason: SetupFailure) -> WindowsInputSetupFailure {
        WindowsInputSetupFailure {
            reason,
            teardown: self.into_teardown(),
        }
    }

    fn into_teardown(mut self) -> WindowsInputTeardown {
        self.startup.take();
        self.abandoned = true;
        WindowsInputTeardown::stopping(&self.control, self.completion.take(), self.thread.take())
    }
}

impl Drop for WindowsInputPendingSubscription {
    fn drop(&mut self) {
        if !self.abandoned {
            self.control.request_stop();
        }
    }
}

impl WindowsInputTeardown {
    fn stopping(
        control: &ThreadControl,
        completion: Option<Receiver<Result<(), TeardownError>>>,
        thread: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            completion,
            thread,
            completion_result: None,
            wake_error: control.request_stop(),
            joined: false,
        }
    }

    pub(in crate::service::local_recording::platform) fn wait_blocking(
        &mut self,
    ) -> Result<TeardownAcknowledgement, TeardownError> {
        let deadline = Instant::now() + TEARDOWN_DEADLINE;
        if !self.joined {
            if self.completion_result.is_none() {
                let completion = self
                    .completion
                    .as_ref()
                    .ok_or(TeardownError::CompletionLost)?;
                self.completion_result = Some(receive_completion(
                    completion,
                    deadline.saturating_duration_since(Instant::now()),
                )?);
            }
            let thread = self.thread.as_ref().ok_or(TeardownError::ThreadPanicked)?;
            if !wait_for_thread_exit(thread, deadline.saturating_duration_since(Instant::now()))? {
                return Err(TeardownError::ThreadExitTimeout);
            }
            self.thread
                .take()
                .ok_or(TeardownError::ThreadPanicked)?
                .join()
                .map_err(|_| TeardownError::ThreadPanicked)?;
            self.joined = true;
            self.completion.take();
        }
        teardown_acknowledgement(
            self.wake_error,
            self.completion_result
                .as_ref()
                .copied()
                .ok_or(TeardownError::CompletionLost)?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service::local_recording::platform) struct TeardownAcknowledgement {
    pub wake_error: Option<u32>,
}

#[derive(Default)]
pub(in crate::service::local_recording::platform) struct ThreadControl {
    pub(super) stop_requested: AtomicBool,
    pub(super) thread_id: AtomicU32,
}

impl ThreadControl {
    pub(in crate::service::local_recording::platform) fn request_stop(&self) -> Option<u32> {
        self.stop_requested.store(true, Ordering::Release);
        let thread_id = self.thread_id.load(Ordering::Acquire);
        request_stop_wake(thread_id, |thread_id| unsafe {
            (PostThreadMessageW(thread_id, WM_QUIT, 0, 0) != 0)
                .then_some(())
                .ok_or_else(last_error_code)
        })
    }
}

pub(in crate::service::local_recording::platform) fn request_stop_wake(
    thread_id: u32,
    post: impl FnOnce(u32) -> Result<(), u32>,
) -> Option<u32> {
    (thread_id != 0).then(|| post(thread_id).err()).flatten()
}

pub(in crate::service::local_recording::platform) fn setup_entry_result(
    stop_requested: bool,
) -> Result<(), SetupFailure> {
    if stop_requested {
        Err(SetupFailure::Stopped)
    } else {
        Ok(())
    }
}

pub(in crate::service::local_recording::platform) fn receive_completion(
    completion: &Receiver<Result<(), TeardownError>>,
    timeout: Duration,
) -> Result<Result<(), TeardownError>, TeardownError> {
    match completion.recv_timeout(timeout) {
        Ok(result) => Ok(result),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(TeardownError::CompletionTimeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(TeardownError::CompletionLost),
    }
}

pub(in crate::service::local_recording::platform) fn teardown_acknowledgement(
    wake_error: Option<u32>,
    release: Result<(), TeardownError>,
) -> Result<TeardownAcknowledgement, TeardownError> {
    release.map(|()| TeardownAcknowledgement { wake_error })
}

fn wait_for_thread_exit(thread: &JoinHandle<()>, timeout: Duration) -> Result<bool, TeardownError> {
    let millis = timeout.as_millis().min(u32::MAX as u128) as u32;
    match unsafe { WaitForSingleObject(thread.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(TeardownError::ThreadWaitFailed(last_error_code())),
        _ => Err(TeardownError::ThreadWaitFailed(last_error_code())),
    }
}
