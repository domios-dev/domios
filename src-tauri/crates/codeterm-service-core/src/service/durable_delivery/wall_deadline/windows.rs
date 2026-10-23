use std::sync::Arc;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateWaitableTimerW, SetEvent, SetWaitableTimer, WaitForMultipleObjects,
    INFINITE,
};

struct Handles {
    timer: usize,
    cancel: usize,
}
impl Drop for Handles {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.timer as HANDLE);
            CloseHandle(self.cancel as HANDLE);
        }
    }
}

pub(in super::super) struct WallDeadline(Arc<Handles>);

impl WallDeadline {
    pub(in super::super) fn arm(
        deadline_ms: u64,
        callback: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let due = deadline_ms
            .checked_add(11_644_473_600_000)
            .and_then(|value| value.checked_mul(10_000))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or("wall deadline overflow")?;
        let handles = unsafe {
            let timer = CreateWaitableTimerW(std::ptr::null(), 1, std::ptr::null());
            if timer.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let cancel = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            if cancel.is_null() {
                CloseHandle(timer);
                return Err(std::io::Error::last_os_error().to_string());
            }
            Arc::new(Handles {
                timer: timer as usize,
                cancel: cancel as usize,
            })
        };
        if unsafe { SetWaitableTimer(handles.timer as HANDLE, &due, 0, None, std::ptr::null(), 0) }
            == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let waiting = handles.clone();
        std::thread::Builder::new()
            .name("durable-result-deadline".into())
            .spawn(move || {
                let events = [waiting.timer as HANDLE, waiting.cancel as HANDLE];
                if unsafe { WaitForMultipleObjects(2, events.as_ptr(), 0, INFINITE) }
                    == WAIT_OBJECT_0
                {
                    callback();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self(handles))
    }
}

impl Drop for WallDeadline {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.0.cancel as HANDLE);
        }
    }
}
