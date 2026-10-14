// App restart hands its successor these stdio handles; a launcher pipe that closes with the
// predecessor makes any later stderr print panic inside the successor.
pub fn detach_for_restart() {
    #[cfg(unix)]
    {
        // SAFETY: plain fd syscalls on a freshly opened descriptor; failures leave stdio as is.
        unsafe {
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
            if null >= 0 {
                libc::dup2(null, libc::STDOUT_FILENO);
                libc::dup2(null, libc::STDERR_FILENO);
                libc::close(null);
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        use windows_sys::Win32::System::Console::{
            SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        };
        if let Ok(null) = std::fs::OpenOptions::new().write(true).open(r"\\.\NUL") {
            let handle = null.into_raw_handle();
            // SAFETY: the handle is leaked on purpose so it outlives the process's std handles.
            unsafe {
                SetStdHandle(STD_OUTPUT_HANDLE, handle as _);
                SetStdHandle(STD_ERROR_HANDLE, handle as _);
            }
        }
    }
}
