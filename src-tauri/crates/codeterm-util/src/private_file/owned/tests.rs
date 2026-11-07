use super::*;
use std::io::{Read, Write};

#[test]
fn private_directory_creation_is_exclusive_and_checked_by_handle() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("credentials");
    create_dir_owner_only(&path).unwrap();
    assert!(open_directory_owner_only(&path).is_ok());
    assert!(create_dir_owner_only(&path).is_err());
    assert!(open_owner_only(&path, 8192).is_err());
    let file = root.path().join("file");
    create_new_owner_only(&file).unwrap();
    assert!(open_directory_owner_only(&file).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(open_directory_owner_only(&path).is_err());
    }
}

#[test]
fn creation_is_exclusive_and_owner_only_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("launch.json");
    let mut file = create_new_owner_only(&path).unwrap();
    assert_eq!(file.metadata().unwrap().len(), 0);
    validate(&file, 0).unwrap();
    assert!(create_new_owner_only(&path).is_err());
    file.write_all(b"launch").unwrap();
    drop(file);
    let mut read = open_owner_only(&path, 6).unwrap();
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"launch");
}

#[test]
fn validation_and_read_stay_on_the_opened_handle_after_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("launch.json");
    create_new_owner_only(&path)
        .unwrap()
        .write_all(b"original")
        .unwrap();
    let mut opened = open_no_follow(&path).unwrap();
    std::fs::rename(&path, root.path().join("old.json")).unwrap();
    std::fs::create_dir(&path).unwrap();
    validate(&opened, 8).unwrap();
    assert!(open_owner_only(&path, 8).is_err());
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
}

#[test]
fn oversized_and_directory_handles_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("launch.json");
    create_new_owner_only(&path)
        .unwrap()
        .write_all(&vec![0; 8193])
        .unwrap();
    assert!(open_owner_only(&path, 8192).is_err());
    assert!(open_owner_only(root.path(), 8192).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_and_fifo_handles_are_refused_without_blocking() {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("launch.json");
    create_new_owner_only(&path).unwrap();
    let link = root.path().join("link.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(open_owner_only(&link, 8192).is_err());
    let fifo = root.path().join("fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let _reader = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .unwrap();
    let _writer = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .unwrap();
    let fifo_handle = open_no_follow(&fifo).unwrap();
    let flags = unsafe { libc::fcntl(fifo_handle.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::O_NONBLOCK, 0);
    assert!(validate(&fifo_handle, 8192).is_err());
    assert!(open_owner_only(&fifo, 8192).is_err());
}

#[cfg(unix)]
#[test]
fn exact_handle_metadata_refuses_foreign_uid_and_public_modes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let file = create_new_owner_only(&root.path().join("launch.json")).unwrap();
    let metadata = file.metadata().unwrap();
    assert!(unix_owned(&metadata, metadata.uid(), 0o600));
    assert!(!unix_owned(
        &metadata,
        metadata.uid().wrapping_add(1),
        0o600
    ));
    file.set_permissions(std::fs::Permissions::from_mode(0o644))
        .unwrap();
    assert!(validate(&file, 8192).is_err());
}

#[cfg(windows)]
#[test]
fn reparse_handles_are_opened_without_following_and_refused() {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("junction");
    std::fs::create_dir(&target).unwrap();
    let mut command = std::process::Command::new("cmd");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let handle = open_no_follow(&link).unwrap();
    assert_ne!(
        handle.metadata().unwrap().file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT,
        0
    );
    assert!(validate(&handle, 8192).is_err());
    assert!(open_owner_only(&link, 8192).is_err());
}
