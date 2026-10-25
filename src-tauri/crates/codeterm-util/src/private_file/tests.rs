use super::*;

#[cfg(unix)]
#[test]
fn restrict_if_present_follows_symlinks_to_regular_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::write(&target, "private").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&target, &link).unwrap();
    restrict_if_present(&link).unwrap();
    assert!(owner_only(&target).unwrap());
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
}

#[test]
fn restrict_if_present_rejects_directories_and_accepts_missing_files() {
    let root = tempfile::tempdir().unwrap();
    assert!(restrict_if_present(root.path()).is_err());
    assert!(restrict_if_present(&root.path().join("missing")).is_ok());
}

#[test]
fn atomic_state_files_and_replacements_are_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_atomic(&path, b"first").unwrap();
    assert!(owner_only(&path).unwrap());
    write_atomic(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert!(owner_only(&path).unwrap());
}

#[test]
fn startup_tightens_existing_state_and_backups() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("state-backups/config.toml");
    fs::create_dir_all(&backup).unwrap();
    let file = backup.join("old.bak");
    fs::write(&file, "sensitive").unwrap();
    crate::sensitive_state::tighten_existing(dir.path());
    assert!(owner_only(&file).unwrap());
}

#[test]
fn failed_atomic_publish_leaves_no_temporary_secret() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("existing-directory");
    fs::create_dir(&target).unwrap();
    assert!(write_atomic(&target, b"sensitive").is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn startup_preserves_owner_execute_and_ignores_external_links() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("public");
    fs::write(&external, "external").unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o644)).unwrap();
    let executable = dir.path().join("tool");
    fs::write(&executable, "executable").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(outside.path(), dir.path().join("link")).unwrap();
    crate::sensitive_state::tighten_existing(dir.path());
    assert_eq!(
        fs::metadata(executable).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(external).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[cfg(windows)]
#[test]
fn windows_private_files_refuse_extra_aces() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("token");
    write_atomic(&file, b"secret").unwrap();
    windows::add_deny_ace_for_test(&file).unwrap();
    assert!(!owner_only(&file).unwrap());
}

#[test]
fn empty_parent_keeps_relative_file_writes_in_the_current_directory() {
    assert!(create_dir_all(Path::new("")).is_ok());
}

#[cfg(unix)]
#[test]
fn private_writes_preserve_the_umask_observed_by_spawned_children() {
    use std::os::unix::fs::PermissionsExt;
    let child_umask = || {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", "umask"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };
    let original = child_umask();
    let root = tempfile::tempdir().unwrap();
    write_atomic(&root.path().join("credential"), b"private").unwrap();
    assert_eq!(child_umask(), original);
    let public = root.path().join("child-output");
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "printf output > \"$1\"", "child"])
        .arg(&public)
        .status()
        .unwrap();
    assert!(child.success());
    let mask = u32::from_str_radix(original.trim(), 8).unwrap();
    assert_eq!(
        fs::metadata(public).unwrap().permissions().mode() & 0o777,
        0o666 & !mask
    );
}
