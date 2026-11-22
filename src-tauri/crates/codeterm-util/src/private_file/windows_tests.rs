use super::*;
use std::mem::size_of;
use std::ptr::null_mut;
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_GROUP,
};
use windows_sys::Win32::Security::{
    CreateWellKnownSid, EqualSid, GetAce, IsWellKnownSid, WinBuiltinAdministratorsSid, WinWorldSid,
    ACCESS_ALLOWED_ACE, OWNER_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    WELL_KNOWN_SID_TYPE,
};

fn grant(path: &Path, kind: WELL_KNOWN_SID_TYPE) {
    let mut length = 0;
    unsafe { CreateWellKnownSid(kind, null_mut(), null_mut(), &mut length) };
    assert!(length > 0);
    let mut storage = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let sid = storage.as_mut_ptr().cast();
    assert_ne!(
        unsafe { CreateWellKnownSid(kind, null_mut(), sid, &mut length) },
        0
    );
    let user = windows::CurrentUser::open().unwrap();
    let mut entry = windows::access_entry(&user, GRANT_ACCESS);
    entry.Trustee.ptstrName = sid.cast();
    entry.Trustee.TrusteeType = TRUSTEE_IS_GROUP;
    if path.is_dir() {
        entry.grfInheritance = SUB_CONTAINERS_AND_OBJECTS_INHERIT;
    }
    let (_descriptor, acl) = windows::read_dacl(path).unwrap();
    windows::write_acl(path, &entry, acl).unwrap();
}

fn masks(path: &Path, kind: WELL_KNOWN_SID_TYPE) -> Vec<u32> {
    let (_descriptor, acl) = windows::read_dacl(path).unwrap();
    let mut masks = Vec::new();
    for index in 0..u32::from(unsafe { (*acl).AceCount }) {
        let mut ace = null_mut();
        assert_ne!(unsafe { GetAce(acl, index, &mut ace) }, 0);
        let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = (&entry.SidStart as *const u32).cast_mut().cast();
        if entry.Header.AceType == 0 && unsafe { IsWellKnownSid(sid, kind) } != 0 {
            masks.push(entry.Mask);
        }
    }
    masks.sort();
    masks
}

#[test]
fn tightening_preserves_explicit_grants_and_removes_broad_access() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    write_atomic(&path, b"before").unwrap();
    grant(&path, WinBuiltinAdministratorsSid);
    grant(&path, WinWorldSid);
    let explicit = masks(&path, WinBuiltinAdministratorsSid);
    assert!(!explicit.is_empty());
    restrict(&path).unwrap();
    assert_eq!(masks(&path, WinBuiltinAdministratorsSid), explicit);
    assert!(masks(&path, WinWorldSid).is_empty());
    write_atomic(&path, b"after").unwrap();
    assert_eq!(masks(&path, WinBuiltinAdministratorsSid), explicit);
}

#[test]
fn generic_replacement_preserves_existing_custom_and_broad_grants() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("provider-config.toml");
    fs::write(&path, b"before").unwrap();
    grant(&path, WinBuiltinAdministratorsSid);
    grant(&path, WinWorldSid);
    let admins = masks(&path, WinBuiltinAdministratorsSid);
    let world = masks(&path, WinWorldSid);
    crate::atomic_write::write_atomic(&path, b"after").unwrap();
    assert_eq!(masks(&path, WinBuiltinAdministratorsSid), admins);
    assert_eq!(masks(&path, WinWorldSid), world);
}

#[test]
fn secret_store_open_and_sensitive_file_repair_do_not_rewrite_root_grants() {
    let root = tempfile::tempdir().unwrap();
    grant(root.path(), WinBuiltinAdministratorsSid);
    let before = masks(root.path(), WinBuiltinAdministratorsSid);
    let store = crate::secret_file::SecretFile::new_at(root.path(), "test");
    store.set("key", "value").unwrap();
    let _again = crate::secret_file::SecretFile::new_at(root.path(), "other");
    crate::sensitive_state::tighten_existing(root.path());
    assert_eq!(masks(root.path(), WinBuiltinAdministratorsSid), before);
}

#[test]
fn a_new_secret_lock_is_owner_only_under_a_broad_inheritable_root() {
    let root = tempfile::tempdir().unwrap();
    grant(root.path(), WinWorldSid);
    let before = masks(root.path(), WinWorldSid);
    let path = root.path().join(".secrets.lock");
    let lock = open_lock(&path).unwrap();
    assert!(owner_only(&path).unwrap());
    assert_eq!(masks(root.path(), WinWorldSid), before);
    drop(lock);
}

#[test]
fn launch_file_is_owner_only_at_creation_under_a_broad_inheritable_root() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    grant(root.path(), WinWorldSid);
    let before = masks(root.path(), WinWorldSid);
    let path = root.path().join("launch.json");
    let mut file = create_new_owner_only(&path).unwrap();
    assert_eq!(file.metadata().unwrap().len(), 0);
    assert!(owner_only(&path).unwrap());
    assert!(masks(&path, WinWorldSid).is_empty());
    assert_eq!(masks(root.path(), WinWorldSid), before);
    file.write_all(b"tab-credential").unwrap();
    drop(file);
    assert!(open_owner_only(&path, 8192).is_ok());
    grant(&path, WinWorldSid);
    assert!(open_owner_only(&path, 8192).is_err());
}

#[test]
fn launch_directory_is_private_at_creation_under_a_broad_inheritable_root() {
    let root = tempfile::tempdir().unwrap();
    grant(root.path(), WinWorldSid);
    let before = masks(root.path(), WinWorldSid);
    let path = root.path().join("launch-credentials");
    create_dir_owner_only(&path).unwrap();
    assert!(open_directory_owner_only(&path).is_ok());
    assert!(masks(&path, WinWorldSid).is_empty());
    assert_eq!(masks(root.path(), WinWorldSid), before);
}

#[test]
fn private_directory_tightening_preserves_explicit_grants_and_removes_broad_access() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("secrets");
    fs::create_dir(&path).unwrap();
    grant(&path, WinBuiltinAdministratorsSid);
    grant(&path, WinWorldSid);
    let explicit = masks(&path, WinBuiltinAdministratorsSid);
    restrict_dir(&path).unwrap();
    assert_eq!(masks(&path, WinBuiltinAdministratorsSid), explicit);
    assert!(masks(&path, WinWorldSid).is_empty());
}

fn assert_token_default_owner(path: &Path) {
    let user = windows::CurrentUser::open().unwrap();
    let wide = windows::wide_path(path);
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(result, 0);
    let _guard = windows::LocalAllocation(descriptor);
    assert!(!owner.is_null());
    let matches_token_owner =
        unsafe { EqualSid(owner, user.sid()) != 0 || EqualSid(owner, user.owner_sid()) != 0 };
    assert!(matches_token_owner);
}

#[test]
fn private_file_operations_enforce_dacl_without_reassigning_owner() {
    let root = tempfile::tempdir().unwrap();
    grant(root.path(), WinWorldSid);

    let dir = root.path().join("private-dir");
    create_dir_owner_only(&dir).unwrap();
    assert!(open_directory_owner_only(&dir).is_ok());
    assert!(masks(&dir, WinWorldSid).is_empty());
    assert_token_default_owner(&dir);

    let atomic = dir.join("credentials.json");
    write_atomic(&atomic, b"initial").unwrap();
    assert!(owner_only(&atomic).unwrap());
    assert!(masks(&atomic, WinWorldSid).is_empty());
    assert_token_default_owner(&atomic);

    grant(&atomic, WinWorldSid);
    write_atomic(&atomic, b"replaced").unwrap();
    assert!(masks(&atomic, WinWorldSid).is_empty());
    assert_token_default_owner(&atomic);

    let tightened = root.path().join("tightened.json");
    fs::write(&tightened, b"secret").unwrap();
    grant(&tightened, WinWorldSid);
    restrict(&tightened).unwrap();
    assert!(masks(&tightened, WinWorldSid).is_empty());
    assert_token_default_owner(&tightened);

    let tightened_dir = root.path().join("tightened-dir");
    fs::create_dir(&tightened_dir).unwrap();
    grant(&tightened_dir, WinWorldSid);
    restrict_dir(&tightened_dir).unwrap();
    assert!(masks(&tightened_dir, WinWorldSid).is_empty());
    assert_token_default_owner(&tightened_dir);

    let exclusive = root.path().join("exclusive.json");
    let file = create_new_owner_only(&exclusive).unwrap();
    drop(file);
    assert!(owner_only(&exclusive).unwrap());
    assert!(masks(&exclusive, WinWorldSid).is_empty());
    assert_token_default_owner(&exclusive);
}
