/// Backend handle / descriptor count, where the platform exposes one. Evidence
/// only: `notify` exposes no portable per-watcher count to assert against.
pub fn open_handle_count() -> Option<u64> {
    platform_count()
}

#[cfg(windows)]
fn platform_count() -> Option<u64> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count: u32 = 0;
    let ok = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    if ok == 0 {
        None
    } else {
        Some(count as u64)
    }
}

#[cfg(target_os = "linux")]
fn platform_count() -> Option<u64> {
    Some(std::fs::read_dir("/proc/self/fd").ok()?.count() as u64)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_count() -> Option<u64> {
    None
}
