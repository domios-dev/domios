use super::*;
use std::mem::size_of;
use windows_sys::Win32::Security::{CreateWellKnownSid, WinWorldSid};

#[test]
fn descriptor_owner_must_be_the_current_user() {
    let user = CurrentUser::open().unwrap();
    let mut length = 0;
    unsafe { CreateWellKnownSid(WinWorldSid, null_mut(), null_mut(), &mut length) };
    assert!(length > 0);
    let mut storage = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let foreign = storage.as_mut_ptr().cast();
    assert_ne!(
        unsafe { CreateWellKnownSid(WinWorldSid, null_mut(), foreign, &mut length) },
        0
    );
    assert!(same_owner(user.sid(), user.sid()));
    assert!(!same_owner(foreign, user.sid()));
    assert!(!same_owner(null_mut(), user.sid()));
}
