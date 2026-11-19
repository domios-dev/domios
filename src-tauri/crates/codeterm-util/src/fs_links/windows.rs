use std::{
    fs::OpenOptions,
    io,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(
        handle: *mut std::ffi::c_void,
        code: u32,
        input: *const std::ffi::c_void,
        input_size: u32,
        output: *mut std::ffi::c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut std::ffi::c_void,
    ) -> i32;
}

pub(super) fn junction(target: &Path, link: &Path) -> io::Result<()> {
    let target = std::fs::canonicalize(target)?;
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Junction target must be a directory",
        ));
    }
    let canonical = target.as_os_str().to_string_lossy();
    let substitute = canonical
        .strip_prefix(r"\\?\")
        .map(|path| format!(r"\??\{path}"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Junction target must be an absolute native path",
            )
        })?;
    let substitute: Vec<u16> = std::ffi::OsStr::new(&substitute).encode_wide().collect();
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let size = 16 + (substitute.len() + print.len() + 2) * 2;
    if size > 16 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Junction target exceeds reparse data limit",
        ));
    }
    let mut buffer = Vec::with_capacity(size);
    buffer.extend_from_slice(&0xa0000003u32.to_le_bytes());
    for value in [
        (size - 8) as u16,
        0,
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (print.len() * 2) as u16,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    std::fs::create_dir(link)?;
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(link)?;
        let mut returned = 0;
        const FSCTL_SET_REPARSE_POINT: u32 = 0x000900a4;
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    result
}
