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

#[derive(Debug, Default)]
enum PairRelease {
    #[default]
    WaitingForExit,
    ObservationFailed(String),
    SessionGone,
    WaitingForPair,
    ReleasingPair,
    Released,
    PairAlreadyTaken,
    PairPoisoned,
}

/// The child's exit, recorded once by the watcher and waitable by readers that
/// saw the output stream end first.
#[derive(Debug, Default)]
pub struct ChildExit {
    state: Mutex<ExitState>,
    resolved: Condvar,
    pair_release: Mutex<PairRelease>,
}

impl ChildExit {
    fn set_pair_release(&self, phase: PairRelease) {
        *self.pair_release.lock().unwrap_or_else(|p| p.into_inner()) = phase;
        log::debug!(
            "[pty-exit] pair release phase={}",
            self.release_diagnostic()
        );
    }

    pub(super) fn release_diagnostic(&self) -> String {
        let phase = self.pair_release.lock().unwrap_or_else(|p| p.into_inner());
        match &*phase {
            PairRelease::ObservationFailed(error) => format!("ObservationFailed({error})"),
            phase => format!("{phase:?}"),
        }
    }

    fn observed(&self, result: Result<Option<i32>, String>, release: impl FnOnce()) {
        match result {
            Ok(code) if self.record(code) => release(),
            Ok(_) => {}
            Err(error) => {
                self.set_pair_release(PairRelease::ObservationFailed(error));
                self.mark_unobservable();
            }
        }
    }

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
        child_exit.set_pair_release(PairRelease::ObservationFailed(
            "no native exit handle".into(),
        ));
        child_exit.mark_unobservable();
        return;
    };
    let thread_label = label.clone();
    let recorder = child_exit.clone();
    let spawned = std::thread::Builder::new()
        .name("pty-child-exit".into())
        .spawn(move || {
            log::debug!("[pty-exit] native wait started for {thread_label}");
            let result = exit.wait();
            log::debug!("[pty-exit] native wait for {thread_label}: {result:?}");
            recorder.observed(result, || {
                let Some(session) = session.upgrade() else {
                    recorder.set_pair_release(PairRelease::SessionGone);
                    return;
                };
                log::debug!(
                    "[pty-exit] releasing pair for {thread_label}, generation={}",
                    session.generation
                );
                recorder.set_pair_release(PairRelease::WaitingForPair);
                let released = match session.pair.lock() {
                    Ok(mut pair) => pair.take(),
                    Err(_) => {
                        recorder.set_pair_release(PairRelease::PairPoisoned);
                        return;
                    }
                };
                if released.is_none() {
                    recorder.set_pair_release(PairRelease::PairAlreadyTaken);
                    return;
                }
                recorder.set_pair_release(PairRelease::ReleasingPair);
                drop(released);
                recorder.set_pair_release(PairRelease::Released);
                log::debug!(
                    "[pty-exit] pair released for {thread_label}, generation={}",
                    session.generation
                );
            });
        });
    if let Err(error) = spawned {
        log::warn!("[pty-exit] watcher spawn failed for pty child {label}: {error}");
        child_exit.set_pair_release(PairRelease::ObservationFailed(format!(
            "watcher spawn: {error}"
        )));
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
            if ok == 0 || owned.is_null() {
                log::debug!(
                    "[pty-exit] DuplicateHandle failed: {}",
                    std::io::Error::last_os_error()
                );
                return None;
            }
            Some(Self(owned))
        }

        pub(super) fn wait(self) -> Result<Option<i32>, String> {
            // SAFETY: `self.0` is an owned process handle open until drop.
            unsafe {
                let result = WaitForSingleObject(self.0, INFINITE);
                if result != WAIT_OBJECT_0 {
                    return Err(format!(
                        "WaitForSingleObject result={result:#x}: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                let mut code = 0u32;
                if GetExitCodeProcess(self.0, &mut code) == 0 {
                    log::debug!(
                        "[pty-exit] GetExitCodeProcess after signaled exit: {}",
                        std::io::Error::last_os_error()
                    );
                    return Ok(None);
                }
                Ok(Some(code as i32))
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
        pub(super) fn wait(self) -> Result<Option<i32>, String> {
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
                    return Ok(Some(if info.si_code == libc::CLD_EXITED {
                        status
                    } else {
                        128 + status
                    }));
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(format!("waitid: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
