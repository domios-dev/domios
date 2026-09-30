//! Observes a PTY child's exit without owning or reaping it. While the session
//! still holds the pseudo-terminal, a shell exiting never ends the output
//! stream, so the watcher records the exit code and closes the pseudo-terminal;
//! the reader's EOF then drives the normal PTY death path.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::manager::PtySession;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ExitState {
    #[default]
    Pending,
    Unobservable,
    Recorded(Option<i32>),
}

/// The child's exit, recorded once by the watcher and waitable by readers that
/// saw the output stream end first.
#[derive(Debug, Default)]
pub struct ChildExit {
    state: Mutex<ExitState>,
    resolved: Condvar,
}

impl ChildExit {
    fn state(&self) -> MutexGuard<'_, ExitState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn outcome(state: ExitState) -> Option<Option<i32>> {
        match state {
            ExitState::Pending => None,
            ExitState::Unobservable => Some(None),
            ExitState::Recorded(code) => Some(code),
        }
    }

    /// Records the exit exactly once and wakes every waiter; later calls are refused.
    pub fn record(&self, code: Option<i32>) -> bool {
        let mut state = self.state();
        if matches!(*state, ExitState::Recorded(_)) {
            return false;
        }
        *state = ExitState::Recorded(code);
        self.resolved.notify_all();
        true
    }

    /// No watcher can report this child's exit, so nobody should wait for it.
    pub fn mark_unobservable(&self) {
        let mut state = self.state();
        if *state == ExitState::Pending {
            *state = ExitState::Unobservable;
            self.resolved.notify_all();
        }
    }

    /// `Some(code)` once the exit is recorded or known to be unobservable.
    pub fn resolved(&self) -> Option<Option<i32>> {
        Self::outcome(*self.state())
    }

    pub fn code(&self) -> Option<i32> {
        self.resolved().flatten()
    }

    /// Waits for the exit to resolve; `None` when `deadline` elapsed first, never a success.
    pub fn wait_resolved(&self, deadline: Duration) -> Option<Option<i32>> {
        let state = self.state();
        let (state, _) = self
            .resolved
            .wait_timeout_while(state, deadline, |state| *state == ExitState::Pending)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::outcome(*state)
    }
}

pub(super) fn watch(
    child: &dyn portable_pty::Child,
    session: Weak<PtySession>,
    child_exit: Arc<ChildExit>,
    label: String,
) {
    let Some(exit) = platform::ExitWait::for_child(child) else {
        log::warn!("[pty-exit] no exit handle for pty child {label}; exit stays unobserved");
        child_exit.mark_unobservable();
        return;
    };
    let thread_label = label.clone();
    let recorder = child_exit.clone();
    let spawned = std::thread::Builder::new()
        .name("pty-child-exit".into())
        .spawn(move || {
            let code = exit.wait();
            if !recorder.record(code) {
                return;
            }
            log::info!("[pty-exit] child of pty {thread_label} exited code={code:?}");
            let Some(session) = session.upgrade() else {
                return;
            };
            let released = session.pair.lock().map(|mut pair| pair.take());
            drop(released);
        });
    if let Err(error) = spawned {
        log::warn!("[pty-exit] watcher spawn failed for pty child {label}: {error}");
        child_exit.mark_unobservable();
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::{
        CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };

    /// An owned duplicate of the child's process handle, so waiting never
    /// touches the `Child` the session's teardown path owns.
    pub(super) struct ExitWait(HANDLE);

    // SAFETY: a process handle is a kernel object reference, valid on any thread.
    unsafe impl Send for ExitWait {}

    impl ExitWait {
        pub(super) fn for_child(child: &dyn portable_pty::Child) -> Option<Self> {
            let source = child.as_raw_handle()? as HANDLE;
            let mut owned: HANDLE = std::ptr::null_mut();
            // SAFETY: `source` is the live child's process handle; the duplicate is ours to close.
            let ok = unsafe {
                let process = GetCurrentProcess();
                DuplicateHandle(
                    process,
                    source,
                    process,
                    &mut owned,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            (ok != 0 && !owned.is_null()).then_some(Self(owned))
        }

        pub(super) fn wait(self) -> Option<i32> {
            // SAFETY: `self.0` is an owned process handle open until drop.
            unsafe {
                if WaitForSingleObject(self.0, INFINITE) != WAIT_OBJECT_0 {
                    return None;
                }
                let mut code = 0u32;
                (GetExitCodeProcess(self.0, &mut code) != 0).then_some(code as i32)
            }
        }
    }

    impl Drop for ExitWait {
        fn drop(&mut self) {
            // SAFETY: closes the duplicate this value owns, exactly once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    pub(super) struct ExitWait(libc::pid_t);

    impl ExitWait {
        pub(super) fn for_child(child: &dyn portable_pty::Child) -> Option<Self> {
            child.process_id().map(|pid| Self(pid as libc::pid_t))
        }

        /// `WNOWAIT` leaves the zombie in place, so teardown's `wait` still reaps it.
        pub(super) fn wait(self) -> Option<i32> {
            loop {
                // SAFETY: zeroed siginfo_t is a valid out-parameter for waitid.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let rc = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.0 as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOWAIT,
                    )
                };
                if rc == 0 {
                    // SAFETY: waitid filled a SIGCHLD record for an exited child.
                    let status = unsafe { info.si_status() };
                    return Some(if info.si_code == libc::CLD_EXITED {
                        status
                    } else {
                        128 + status
                    });
                }
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    return None;
                }
            }
        }
    }
}
