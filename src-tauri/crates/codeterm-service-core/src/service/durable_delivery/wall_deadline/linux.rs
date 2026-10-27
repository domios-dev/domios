use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub(in super::super) struct WallDeadline(tokio::task::AbortHandle);

impl WallDeadline {
    pub(in super::super) fn arm(
        deadline_ms: u64,
        callback: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let seconds =
            libc::time_t::try_from(deadline_ms / 1000).map_err(|_| "wall deadline overflow")?;
        let raw = unsafe {
            libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_CLOEXEC | libc::TFD_NONBLOCK)
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let timer = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: seconds,
                tv_nsec: (deadline_ms % 1000 * 1_000_000) as _,
            },
        };
        if unsafe {
            libc::timerfd_settime(
                fd.as_raw_fd(),
                libc::TFD_TIMER_ABSTIME,
                &timer,
                std::ptr::null_mut(),
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let runtime = crate::async_runtime::handle();
        let _entered = runtime.enter();
        // SAFETY: OwnedFd keeps the same descriptor open until AsyncFd releases it.
        let fd =
            unsafe { tokio::io::unix::AsyncFd::register(fd) }.map_err(|error| error.to_string())?;
        let task = crate::async_runtime::spawn(async move {
            if fd.readable().await.is_ok() {
                callback();
            }
        });
        Ok(Self(task.abort_handle()))
    }
}

impl Drop for WallDeadline {
    fn drop(&mut self) {
        self.0.abort();
    }
}
