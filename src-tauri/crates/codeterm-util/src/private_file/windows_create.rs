use super::windows::{access_entry, wide_path, CurrentUser, LocalAllocation};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{SetEntriesInAclW, GRANT_ACCESS};
use windows_sys::Win32::Security::{
    InitializeSecurityDescriptor, SetSecurityDescriptorControl, SetSecurityDescriptorDacl,
    SetSecurityDescriptorOwner, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_CREATION_DISPOSITION,
    FILE_SHARE_MODE,
};
use windows_sys::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;

pub(super) fn open(
    path: &Path,
    creation: FILE_CREATION_DISPOSITION,
    sharing: FILE_SHARE_MODE,
) -> io::Result<File> {
    with_attributes(0, |attributes| {
        let name = wide_path(path);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                sharing,
                attributes,
                creation,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    })
}

pub(super) fn create_dir(path: &Path) -> io::Result<()> {
    with_attributes(SUB_CONTAINERS_AND_OBJECTS_INHERIT, |attributes| {
        let name = wide_path(path);
        if unsafe { CreateDirectoryW(name.as_ptr(), attributes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })
}

fn with_attributes<T>(
    inheritance: u32,
    create: impl FnOnce(&SECURITY_ATTRIBUTES) -> io::Result<T>,
) -> io::Result<T> {
    let user = CurrentUser::open()?;
    let mut entry = access_entry(&user, GRANT_ACCESS);
    entry.grfInheritance = inheritance;
    let mut acl = null_mut();
    let result = unsafe { SetEntriesInAclW(1, &entry, null(), &mut acl) };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _acl = LocalAllocation(acl.cast());
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let descriptor = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
    if unsafe { InitializeSecurityDescriptor(descriptor, SECURITY_DESCRIPTOR_REVISION) } == 0
        || unsafe { SetSecurityDescriptorOwner(descriptor, user.sid(), 0) } == 0
        || unsafe { SetSecurityDescriptorDacl(descriptor, 1, acl, 0) } == 0
        || unsafe { SetSecurityDescriptorControl(descriptor, SE_DACL_PROTECTED, SE_DACL_PROTECTED) }
            == 0
    {
        return Err(io::Error::last_os_error());
    }
    create(&SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    })
}
