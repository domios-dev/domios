pub(super) fn current() -> codeterm_analytics::Context {
    let locale = locale();
    codeterm_analytics::Context::new(
        codeterm_build_info::version(),
        sysinfo::System::os_version(),
        locale.as_deref(),
    )
}

#[cfg(windows)]
fn locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    }
    let mut name = [0u16; 85];
    // The OS writes at most the supplied UTF-16 buffer length, including its terminator.
    let length = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    (length > 1).then(|| String::from_utf16_lossy(&name[..length as usize - 1]))
}

#[cfg(target_os = "macos")]
fn locale() -> Option<String> {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyCurrent() -> *const c_void;
        fn CFLocaleGetIdentifier(locale: *const c_void) -> *const c_void;
        fn CFStringGetCString(
            value: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: *const c_void);
    }
    // The copied locale owns the identifier until release; conversion is bounded by the buffer.
    unsafe {
        let locale = CFLocaleCopyCurrent();
        if locale.is_null() {
            return None;
        }
        let identifier = CFLocaleGetIdentifier(locale);
        let mut buffer = [0 as c_char; 128];
        let converted = !identifier.is_null()
            && CFStringGetCString(
                identifier,
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                0x0800_0100,
            ) != 0;
        CFRelease(locale);
        converted.then(|| {
            CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned()
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
}
