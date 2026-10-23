use std::io;
use std::mem::size_of;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Security::Authorization::{
    SetNamedSecurityInfoW, GRANT_ACCESS, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AddAce, GetAce, GetSecurityDescriptorControl, InitializeAcl, IsWellKnownSid,
    WinAuthenticatedUserSid, WinBuiltinUsersSid, WinWorldSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
    ACL_REVISION, DACL_SECURITY_INFORMATION, INHERITED_ACE, PROTECTED_DACL_SECURITY_INFORMATION,
    SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};

use super::windows::{access_entry, read_dacl, wide_path, write_acl, CurrentUser};

pub(super) fn tighten(path: &Path) -> io::Result<()> {
    copy_tightened(path, path)
}

pub(super) fn copy_tightened(source: &Path, target: &Path) -> io::Result<()> {
    copy_with_policy(source, target, false)
}

pub(super) fn tighten_directory(path: &Path) -> io::Result<()> {
    copy_with_policy(path, path, true)
}

fn copy_with_policy(source: &Path, target: &Path, directory: bool) -> io::Result<()> {
    let user = CurrentUser::open()?;
    let (_descriptor, original) = read_dacl(source)?;
    let length = if original.is_null() {
        size_of::<ACL>()
    } else {
        unsafe { (*original).AclSize as usize }
    };
    let mut storage = vec![0usize; length.div_ceil(size_of::<usize>())];
    let filtered = storage.as_mut_ptr().cast::<ACL>();
    if unsafe { InitializeAcl(filtered, length as u32, ACL_REVISION) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !original.is_null() {
        for index in 0..u32::from(unsafe { (*original).AceCount }) {
            let mut ace = null_mut();
            if unsafe { GetAce(original, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            if u32::from(header.AceFlags) & INHERITED_ACE != 0 {
                continue;
            }
            if !matches!(
                u32::from(header.AceType),
                ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE
            ) {
                return Err(io::Error::other(
                    "Unsupported explicit ACL entry on sensitive state",
                ));
            }
            let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            let sid = (&entry.SidStart as *const u32).cast_mut().cast();
            let broad = [WinWorldSid, WinBuiltinUsersSid, WinAuthenticatedUserSid]
                .into_iter()
                .any(|kind| unsafe { IsWellKnownSid(sid, kind) } != 0);
            if broad {
                continue;
            }
            if unsafe {
                AddAce(
                    filtered,
                    ACL_REVISION,
                    u32::MAX,
                    ace,
                    u32::from(header.AceSize),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
    }
    let mut owner = access_entry(&user, GRANT_ACCESS);
    if directory {
        owner.grfInheritance = windows_sys::Win32::Security::SUB_CONTAINERS_AND_OBJECTS_INHERIT;
    }
    write_acl(target, &owner, filtered)
}

pub(super) fn copy_permissions(source: &Path, target: &Path) -> io::Result<()> {
    let (descriptor, dacl) = read_dacl(source)?;
    let (mut control, mut revision) = (0u16, 0u32);
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let protection = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let path = wide_path(target);
    let result = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    Ok(())
}
