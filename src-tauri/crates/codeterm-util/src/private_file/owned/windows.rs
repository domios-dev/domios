use crate::private_file::windows::{descriptor_mode_ok, CurrentUser, LocalAllocation};
use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    EqualSid, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(super) fn create_new(path: &Path) -> io::Result<File> {
    crate::private_file::windows_create::open(
        path,
        CREATE_NEW,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

pub(super) fn owner_only(file: &File) -> io::Result<bool> {
    let user = CurrentUser::open()?;
    let mut owner = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let descriptor = LocalAllocation(descriptor);
    if !same_owner(owner, user.sid()) {
        return Ok(false);
    }
    descriptor_mode_ok(&descriptor, dacl, &user)
}

fn same_owner(
    owner: windows_sys::Win32::Security::PSID,
    expected: windows_sys::Win32::Security::PSID,
) -> bool {
    !owner.is_null() && unsafe { EqualSid(owner, expected) } != 0
}

#[cfg(test)]
mod tests;
