pub(super) fn lower() {
    #[cfg(windows)]
    let failed = unsafe {
        windows_sys::Win32::System::Threading::SetThreadPriority(
            windows_sys::Win32::System::Threading::GetCurrentThread(),
            windows_sys::Win32::System::Threading::THREAD_PRIORITY_BELOW_NORMAL,
        ) == 0
    };
    #[cfg(target_os = "linux")]
    let failed = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 10) != 0 };
    #[cfg(target_os = "macos")]
    let failed = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) != 0
    };
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let failed = false;
    if failed {
        log::debug!("[embed] could not lower CPU worker scheduling priority");
    }
}
