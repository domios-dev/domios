use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
#[cfg(test)]
use windows_sys::Win32::Security::Authorization::DENY_ACCESS;
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, ACCESS_MODE, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorControl, GetTokenInformation, TokenUser,
    ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE,
    PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(super) fn restrict(path: &Path) -> io::Result<()> {
    let user = CurrentUser::open()?;
    let entry = access_entry(&user, GRANT_ACCESS);
    write_acl(path, &entry, null())
}

pub(super) fn mode_ok(path: &Path) -> io::Result<bool> {
    let user = CurrentUser::open()?;
    let (_descriptor, dacl) = read_dacl(path)?;
    if dacl.is_null() {
        return Ok(false);
    }

    let (mut control, mut revision) = (0u16, 0u32);
    let descriptor = _descriptor.0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Ok(false);
    }

    let ace_count = unsafe { (*dacl).AceCount };
    if ace_count == 0 {
        return Ok(false);
    }
    for index in 0..ace_count as u32 {
        let mut ace = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if u32::from(allowed.Header.AceType) != ACCESS_ALLOWED_ACE_TYPE
            || allowed.Header.AceFlags != 0
            || allowed.Mask != FILE_ALL_ACCESS
            || unsafe {
                EqualSid(
                    (&allowed.SidStart as *const u32).cast_mut().cast(),
                    user.sid(),
                )
            } == 0
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
pub(super) fn add_deny_ace_for_test(path: &Path) -> io::Result<()> {
    let user = CurrentUser::open()?;
    let (_descriptor, dacl) = read_dacl(path)?;
    let entry = access_entry(&user, DENY_ACCESS);
    write_acl(path, &entry, dacl)
}

fn access_entry(user: &CurrentUser, mode: ACCESS_MODE) -> EXPLICIT_ACCESS_W {
    EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: mode,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: user.sid().cast(),
        },
    }
}

fn write_acl(path: &Path, entry: &EXPLICIT_ACCESS_W, old_acl: *const ACL) -> io::Result<()> {
    let mut acl = null_mut();
    let result = unsafe { SetEntriesInAclW(1, entry, old_acl, &mut acl) };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _acl = LocalAllocation(acl.cast());
    let path = wide_path(path);
    let result = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null_mut(),
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    Ok(())
}

fn read_dacl(path: &Path) -> io::Result<(LocalAllocation, *mut ACL)> {
    let path = wide_path(path);
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    Ok((LocalAllocation(descriptor), dacl))
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct CurrentUser {
    token_info: Vec<usize>,
}

impl CurrentUser {
    fn open() -> io::Result<Self> {
        let mut token: HANDLE = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = TokenHandle(token);
        let mut required = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        }
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        let units = (required as usize + size_of::<usize>() - 1) / size_of::<usize>();
        let mut token_info = vec![0usize; units];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                token_info.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { token_info })
    }

    fn sid(&self) -> windows_sys::Win32::Security::PSID {
        unsafe { (*self.token_info.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}
