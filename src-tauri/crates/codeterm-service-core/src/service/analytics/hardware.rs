#[cfg(windows)]
pub(super) fn device() -> (Option<String>, Option<String>) {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    fn value(name: &str) -> Option<String> {
        let key: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\BIOS\0"
            .encode_utf16()
            .collect();
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut buffer = [0u16; 256];
        let mut bytes = std::mem::size_of_val(&buffer) as u32;
        // RegGetValueW bounds writes by bytes and terminates REG_SZ values on success.
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if result != 0 {
            return None;
        }
        let end = buffer.iter().position(|value| *value == 0)?;
        String::from_utf16(&buffer[..end]).ok()
    }
    (value("SystemManufacturer"), value("SystemProductName"))
}

#[cfg(target_os = "macos")]
pub(super) fn device() -> (Option<String>, Option<String>) {
    let mut buffer = [0u8; 256];
    let mut size = buffer.len();
    // sysctlbyname bounds the OS model string by the supplied buffer size.
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.model".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    let model = (result == 0 && size <= buffer.len()).then(|| {
        String::from_utf8_lossy(&buffer[..size])
            .trim_end_matches('\0')
            .to_owned()
    });
    (Some("Apple".into()), model)
}

#[cfg(target_os = "linux")]
pub(super) fn device() -> (Option<String>, Option<String>) {
    use std::io::Read;
    fn value(path: &str) -> Option<String> {
        let mut value = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(512)
            .read_to_string(&mut value)
            .ok()?;
        Some(value)
    }
    (
        value("/sys/class/dmi/id/sys_vendor"),
        value("/sys/class/dmi/id/product_name"),
    )
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub(super) fn device() -> (Option<String>, Option<String>) {
    (None, None)
}
