use super::*;
use windows_sys::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
    GetSecurityDescriptorOwner, ACCESS_ALLOWED_ACE,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

#[test]
fn creation_uses_token_default_owner_and_protected_user_only_dacl() {
    for inheritance in [0, SUB_CONTAINERS_AND_OBJECTS_INHERIT] {
        with_attributes(inheritance, |attributes| {
            let descriptor = attributes.lpSecurityDescriptor;
            let mut owner = null_mut();
            let mut defaulted = 0;
            assert_ne!(
                unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) },
                0
            );
            assert!(owner.is_null());
            let (mut control, mut revision) = (0, 0);
            assert_ne!(
                unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut present = 0;
            let mut acl = null_mut();
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
                },
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!(unsafe { (*acl).AceCount }, 1);
            let mut ace = null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &mut ace) }, 0);
            let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            assert_eq!(entry.Header.AceType, 0);
            assert_eq!(u32::from(entry.Header.AceFlags), inheritance);
            assert_eq!(entry.Mask, FILE_ALL_ACCESS);
            let user = CurrentUser::open()?;
            assert_ne!(
                unsafe {
                    EqualSid(
                        (&entry.SidStart as *const u32).cast_mut().cast(),
                        user.sid(),
                    )
                },
                0
            );
            Ok(())
        })
        .unwrap();
    }
}
