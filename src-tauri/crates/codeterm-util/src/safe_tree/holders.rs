use std::path::Path;

#[cfg(windows)]
pub(super) fn describe(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
    use windows_sys::Win32::System::RestartManager::*;

    struct Session(u32);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                RmEndSession(self.0);
            }
        }
    }
    let mut id = 0;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let result = unsafe { RmStartSession(&mut id, 0, key.as_mut_ptr()) };
    if result != 0 {
        return format!("holder discovery unavailable (RmStartSession {result})");
    }
    let session = Session(id);
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        RmRegisterResources(
            session.0,
            1,
            &name.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        )
    };
    if result != 0 {
        return format!("holder discovery unavailable (RmRegisterResources {result})");
    }
    let (mut needed, mut count, mut reasons) = (0, 0, 0);
    let result = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            std::ptr::null_mut(),
            &mut reasons,
        )
    };
    if result == 0 {
        return "no filesystem holder reported by Windows".into();
    }
    if result != ERROR_MORE_DATA || needed > 4096 {
        return format!("holder discovery unavailable (RmGetList {result}, count {needed})");
    }
    let mut holders = vec![RM_PROCESS_INFO::default(); needed as usize];
    count = needed;
    let result = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            holders.as_mut_ptr(),
            &mut reasons,
        )
    };
    if result != 0 {
        return format!("holder discovery unavailable (RmGetList {result})");
    }
    holders.truncate(count as usize);
    holders
        .iter()
        .map(|holder| {
            let end = holder
                .strAppName
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(holder.strAppName.len());
            format!(
                "{} (PID {})",
                String::from_utf16_lossy(&holder.strAppName[..end]),
                holder.Process.dwProcessId
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(not(windows))]
pub(super) fn describe(_: &Path) -> String {
    "filesystem holder unavailable on this platform".into()
}
